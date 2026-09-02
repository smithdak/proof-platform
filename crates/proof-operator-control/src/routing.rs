use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    extract::{connect_info::ConnectInfo, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri},
    routing::any,
    Router,
};
use proof_kernel::{
    Capability, CapabilitySet, OperatorCommand, OperatorControlEnvironment, SessionRevokeRequest,
};
use proof_operator_auth::{
    AuthorizedCallError, AuthorizedSession, ChallengeIssueRequest, OperatorAuthAuthority,
    OperatorAuthError, SessionExchangeRequest,
};
use serde::{
    de::{Error as _, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{ControlShellError, LoopbackOrigin, StaticBundle};

const SESSION_HEADER: &str = "x-proof-operator-session";
const TARGET_LIMIT: usize = 2048;
const SESSION_BODY_LIMIT: usize = 4096;
const MUTATION_BODY_LIMIT: usize = 8192;
const RATE_SCALE: u128 = 60_000;
const MAX_PROTECTED_BUCKETS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMethod {
    Get,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountedRoute {
    pub method: RouteMethod,
    pub path: &'static str,
}

const ROUTES: [MountedRoute; 15] = [
    MountedRoute {
        method: RouteMethod::Get,
        path: "/",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/assets/:asset",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/session/challenges",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/session/exchange",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/session/revoke",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/attention",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/runs/:run_id",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/approvals",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/approvals/:request_id",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/commands",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/commands/:command_id",
    },
    MountedRoute {
        method: RouteMethod::Get,
        path: "/operator/v1/audit",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/approvals/:request_id/decisions",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/runs/:run_id/cancel",
    },
    MountedRoute {
        method: RouteMethod::Post,
        path: "/operator/v1/runs/:run_id/resume",
    },
];

pub fn frozen_route_inventory() -> &'static [MountedRoute] {
    &ROUTES
}

/// Strict, redacted request delivered only after the complete shell boundary.
#[derive(Debug, Clone)]
pub struct ProtectedRequest {
    pub route: MountedRoute,
    pub path: String,
    pub query: Option<String>,
    pub command: Option<OperatorCommand>,
    pub session: AuthorizedSession,
}

/// Synchronous injected business boundary. W4's implementation is synthetic;
/// later composition supplies durable read and command handlers.
pub trait OperatorRouteHandler: Send + Sync {
    fn handle(&self, request: ProtectedRequest) -> Result<Response<Body>, ControlShellError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntheticEffectSnapshot {
    pub callbacks: usize,
    pub provider_calls: usize,
    pub tool_calls: usize,
    pub external_effects: usize,
}

/// No-authority fixture handler. It never performs provider, tool, or external
/// work, and makes callback entry independently observable.
#[derive(Default)]
pub struct SyntheticRouteHandler {
    callbacks: AtomicUsize,
    provider_calls: AtomicUsize,
    tool_calls: AtomicUsize,
    external_effects: AtomicUsize,
}

impl SyntheticRouteHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effect_snapshot(&self) -> SyntheticEffectSnapshot {
        SyntheticEffectSnapshot {
            callbacks: self.callbacks.load(Ordering::SeqCst),
            provider_calls: self.provider_calls.load(Ordering::SeqCst),
            tool_calls: self.tool_calls.load(Ordering::SeqCst),
            external_effects: self.external_effects.load(Ordering::SeqCst),
        }
    }
}

impl OperatorRouteHandler for SyntheticRouteHandler {
    fn handle(&self, _request: ProtectedRequest) -> Result<Response<Body>, ControlShellError> {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        Err(ControlShellError::ControlUnavailable)
    }
}

#[derive(Clone)]
pub struct OperatorRouterState {
    static_bundle: Arc<dyn StaticBundle>,
    handler: Arc<dyn OperatorRouteHandler>,
    authority: Arc<OperatorAuthAuthority>,
    environment: Arc<dyn OperatorControlEnvironment>,
    endpoint: LoopbackOrigin,
    rates: Arc<Mutex<RateState>>,
    fatal_shutdown: Arc<AtomicBool>,
}

impl OperatorRouterState {
    pub fn new(
        endpoint: LoopbackOrigin,
        static_bundle: Arc<dyn StaticBundle>,
        handler: Arc<dyn OperatorRouteHandler>,
        authority: Arc<OperatorAuthAuthority>,
        environment: Arc<dyn OperatorControlEnvironment>,
    ) -> Result<Self, ControlShellError> {
        if endpoint.address().ip() != IpAddr::V4(Ipv4Addr::LOCALHOST)
            || endpoint.address().port() == 0
        {
            return Err(ControlShellError::ListenerUnavailable);
        }
        static_bundle.validate()?;
        Ok(Self {
            static_bundle,
            handler,
            authority,
            environment,
            endpoint,
            rates: Arc::new(Mutex::new(RateState::default())),
            fatal_shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn fatal_shutdown_requested(&self) -> bool {
        self.fatal_shutdown.load(Ordering::SeqCst)
    }

    pub async fn wait_for_fatal_shutdown(&self) {
        while !self.fatal_shutdown_requested() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    pub fn origin(&self) -> &str {
        self.endpoint.origin()
    }
}

/// Builds a new empty router around only the frozen inventory and closed static
/// source. Every framework fallback reaches the same hardened dispatcher.
pub fn build_operator_router(state: OperatorRouterState) -> Router {
    Router::new().fallback(any(dispatch)).with_state(state)
}

#[cfg(test)]
pub(crate) async fn dispatch_for_test(
    state: OperatorRouterState,
    request: Request<Body>,
) -> Response<Body> {
    dispatch(State(state), request).await
}

async fn dispatch(
    State(state): State<OperatorRouterState>,
    request: Request<Body>,
) -> Response<Body> {
    let response = dispatch_inner(&state, request).await;
    harden(response)
}

async fn dispatch_inner(state: &OperatorRouterState, request: Request<Body>) -> Response<Body> {
    if !valid_peer_host_target(state, &request) {
        return not_found();
    }
    if state.fatal_shutdown_requested() {
        return unavailable();
    }

    let classification = classify(request.method(), request.uri().path());
    match classification {
        RouteClass::Static => serve_static(state, request.uri().path(), request.uri().query()),
        RouteClass::PublicSession(route) => serve_public_session(state, request, route).await,
        RouteClass::Protected(route) => serve_protected(state, request, route).await,
        RouteClass::ProtectedFallback => serve_protected_fallback(state, &request),
        RouteClass::Absent => not_found(),
    }
}

fn valid_peer_host_target(state: &OperatorRouterState, request: &Request<Body>) -> bool {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|value| value.0);
    let peer_valid = peer.is_some_and(|address| address.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST));
    let host_values: Vec<_> = request.headers().get_all(header::HOST).iter().collect();
    let host_valid =
        host_values.len() == 1 && host_values[0].as_bytes() == state.endpoint.host().as_bytes();
    let target_valid = valid_origin_form(request.uri());
    peer_valid && host_valid && target_valid
}

fn valid_origin_form(uri: &Uri) -> bool {
    uri.scheme().is_none()
        && uri.authority().is_none()
        && uri.path_and_query().is_some_and(|target| {
            let bytes = target.as_str().as_bytes();
            !bytes.is_empty() && bytes[0] == b'/' && bytes.len() <= TARGET_LIMIT
        })
}

enum RouteClass {
    Static,
    PublicSession(MountedRoute),
    Protected(MountedRoute),
    ProtectedFallback,
    Absent,
}

fn classify(method: &Method, path: &str) -> RouteClass {
    if method == Method::GET && (path == "/" || path.starts_with("/assets/")) {
        return RouteClass::Static;
    }
    if let Some(route) = match_route(method, path) {
        if matches!(
            route.path,
            "/operator/v1/session/challenges" | "/operator/v1/session/exchange"
        ) {
            RouteClass::PublicSession(route)
        } else {
            RouteClass::Protected(route)
        }
    } else if path.starts_with("/operator/v1/") {
        RouteClass::ProtectedFallback
    } else {
        RouteClass::Absent
    }
}

fn serve_static(state: &OperatorRouterState, path: &str, query: Option<&str>) -> Response<Body> {
    if query.is_some() {
        return invalid_request();
    }
    match state.static_bundle.asset(path) {
        Some(asset) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, asset.media_type())
            .body(Body::from(asset.bytes().to_vec()))
            .expect("validated static response"),
        None => not_found(),
    }
}

async fn serve_public_session(
    state: &OperatorRouterState,
    request: Request<Body>,
    route: MountedRoute,
) -> Response<Body> {
    let has_query = request.uri().query().is_some();
    if !single_header_equals(request.headers(), header::ORIGIN, state.endpoint.origin()) {
        return invalid_request();
    }
    if !single_header_equals(request.headers(), header::CONTENT_TYPE, "application/json") {
        return unsupported_media_type();
    }
    let body = match collect_body(request.into_body(), SESSION_BODY_LIMIT).await {
        Ok(body) => body,
        Err(BodyFailure::TooLarge) => return request_too_large(),
    };
    let tick = match state.environment.monotonic_millis() {
        Ok(tick) => tick,
        Err(_) => return fatal_unavailable(state),
    };
    let rate = if route.path == "/operator/v1/session/challenges" {
        PublicRate::Challenge
    } else {
        PublicRate::Exchange
    };
    match consume_public_rate(state, rate, tick) {
        Ok(true) => {}
        Ok(false) => return rate_limited(),
        Err(()) => return fatal_unavailable(state),
    }
    if has_query {
        return invalid_request();
    }

    if rate == PublicRate::Challenge {
        let request: ChallengeIssueRequest = match strict_decode(&body) {
            Ok(request) => request,
            Err(()) => return invalid_request(),
        };
        match state.authority.issue_challenge(request) {
            Ok(response) => json_response(StatusCode::CREATED, &response),
            Err(OperatorAuthError::InvalidRequest) => invalid_request(),
            Err(OperatorAuthError::ChallengePending) => unavailable(),
            Err(OperatorAuthError::ControlUnavailable) => fatal_unavailable(state),
            Err(_) => unavailable(),
        }
    } else {
        let request: SessionExchangeRequest = match strict_decode(&body) {
            Ok(request) => request,
            Err(()) => return invalid_request(),
        };
        match state.authority.exchange(request) {
            Ok(response) => json_response(StatusCode::CREATED, &response),
            Err(OperatorAuthError::ControlUnavailable) => fatal_unavailable(state),
            Err(_) => authentication_required(),
        }
    }
}

async fn serve_protected(
    state: &OperatorRouterState,
    request: Request<Body>,
    route: MountedRoute,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let header_values = session_header_values(&parts.headers);
    let initial = match authorize_any(&state.authority, &header_values) {
        Ok(session) => session,
        Err(error) => return auth_error(state, error),
    };
    let tick = match state.environment.monotonic_millis() {
        Ok(tick) => tick,
        Err(_) => return fatal_unavailable(state),
    };
    match consume_protected_rate(state, initial.session_id, tick) {
        Ok(true) => {}
        Ok(false) => return rate_limited(),
        Err(()) => return fatal_unavailable(state),
    }

    let required = if route.path == "/operator/v1/attention" {
        match attention_capabilities(parts.uri.query()) {
            Ok(required) => required,
            Err(()) => return invalid_request(),
        }
    } else {
        fixed_capabilities(route)
    };
    let authorized = match authorize_required(&state.authority, &header_values, &required) {
        Ok(session) => session,
        Err(error) => return auth_error(state, error),
    };
    let path = parts.uri.path().to_owned();
    let query = parts.uri.query().map(str::to_owned);

    let command = match route.method {
        RouteMethod::Get => {
            if parts.headers.contains_key(header::TRANSFER_ENCODING)
                || parts
                    .headers
                    .get(header::CONTENT_LENGTH)
                    .is_some_and(|value| value.as_bytes() != b"0")
                || collect_body(body, 0).await.is_err()
                || validate_get_target(route, &path, query.as_deref()).is_err()
            {
                return invalid_request();
            }
            None
        }
        RouteMethod::Post => {
            if !single_header_equals(&parts.headers, header::ORIGIN, state.endpoint.origin()) {
                return invalid_request();
            }
            if !single_header_equals(&parts.headers, header::CONTENT_TYPE, "application/json") {
                return unsupported_media_type();
            }
            let body = match collect_body(body, MUTATION_BODY_LIMIT).await {
                Ok(body) => body,
                Err(BodyFailure::TooLarge) => return request_too_large(),
            };
            if query.is_some() {
                return invalid_request();
            }
            match decode_command(route, &path, &authorized, &body) {
                Ok(command) => Some(command),
                Err(()) => return invalid_request(),
            }
        }
    };

    let protected = ProtectedRequest {
        route,
        path,
        query,
        command,
        session: authorized,
    };
    invoke_handler(state, &header_values, required, protected)
}

fn serve_protected_fallback(
    state: &OperatorRouterState,
    request: &Request<Body>,
) -> Response<Body> {
    let header_values = session_header_values(request.headers());
    match authorize_any(&state.authority, &header_values) {
        Ok(session) => {
            let tick = match state.environment.monotonic_millis() {
                Ok(tick) => tick,
                Err(_) => return fatal_unavailable(state),
            };
            match consume_protected_rate(state, session.session_id, tick) {
                Ok(true) => not_found(),
                Ok(false) => rate_limited(),
                Err(()) => fatal_unavailable(state),
            }
        }
        Err(error) => auth_error(state, error),
    }
}

fn invoke_handler(
    state: &OperatorRouterState,
    header_values: &[&[u8]],
    required: Vec<Capability>,
    request: ProtectedRequest,
) -> Response<Body> {
    let revoke = request.route.path == "/operator/v1/session/revoke";
    let result = if revoke {
        state.authority.revoke_with(header_values, |_| {
            let response = state.handler.handle(request)?;
            if response.status() != StatusCode::OK {
                return Err(ControlShellError::ControlUnavailable);
            }
            Ok(response)
        })
    } else {
        let required = CapabilitySet::new(required)
            .expect("every non-revoke protected route has canonical capabilities");
        state
            .authority
            .authorize_with(header_values, &required, |_| state.handler.handle(request))
    };
    match result {
        Ok(response) if response.status() == StatusCode::SERVICE_UNAVAILABLE => {
            fatal_unavailable(state)
        }
        Ok(response) => response,
        Err(AuthorizedCallError::Auth(error)) => auth_error(state, error),
        Err(AuthorizedCallError::Callback(_)) => fatal_unavailable(state),
    }
}

fn authorize_any(
    authority: &OperatorAuthAuthority,
    values: &[&[u8]],
) -> Result<AuthorizedSession, OperatorAuthError> {
    match authority.authorize_any_with(values, |session| {
        Ok::<AuthorizedSession, ()>(session.clone())
    }) {
        Ok(session) => Ok(session),
        Err(AuthorizedCallError::Auth(error)) => Err(error),
        Err(AuthorizedCallError::Callback(())) => Err(OperatorAuthError::ControlUnavailable),
    }
}

fn authorize_required(
    authority: &OperatorAuthAuthority,
    values: &[&[u8]],
    required: &[Capability],
) -> Result<AuthorizedSession, OperatorAuthError> {
    if required.is_empty() {
        return authorize_any(authority, values);
    }
    let required =
        CapabilitySet::new(required.to_vec()).map_err(|_| OperatorAuthError::InvalidRequest)?;
    match authority.authorize_with(values, &required, |session| {
        Ok::<AuthorizedSession, ()>(session.clone())
    }) {
        Ok(session) => Ok(session),
        Err(AuthorizedCallError::Auth(error)) => Err(error),
        Err(AuthorizedCallError::Callback(())) => Err(OperatorAuthError::ControlUnavailable),
    }
}

fn auth_error(state: &OperatorRouterState, error: OperatorAuthError) -> Response<Body> {
    match error {
        OperatorAuthError::CapabilityRequired => capability_required(),
        OperatorAuthError::ControlUnavailable => fatal_unavailable(state),
        _ => authentication_required(),
    }
}

fn fixed_capabilities(route: MountedRoute) -> Vec<Capability> {
    match route.path {
        "/operator/v1/session/revoke" => Vec::new(),
        "/operator/v1/runs/:run_id" => vec![Capability::RunRead],
        "/operator/v1/approvals" | "/operator/v1/approvals/:request_id" => {
            vec![Capability::ApprovalRead]
        }
        "/operator/v1/commands" | "/operator/v1/commands/:command_id" | "/operator/v1/audit" => {
            vec![Capability::AuditRead]
        }
        "/operator/v1/approvals/:request_id/decisions" => {
            vec![Capability::ApprovalDecide, Capability::ApprovalRead]
        }
        "/operator/v1/runs/:run_id/cancel" => {
            vec![Capability::RunCancel, Capability::RunRead]
        }
        "/operator/v1/runs/:run_id/resume" => {
            vec![Capability::RunRead, Capability::RunResume]
        }
        _ => unreachable!("route inventory is closed"),
    }
}

fn attention_capabilities(query: Option<&str>) -> Result<Vec<Capability>, ()> {
    let query = query.ok_or(())?;
    let mut pairs = query.split('&');
    let (name, value) = decode_pair(pairs.next().ok_or(())?)?;
    if name != "schema" || value != "proof.operator.attention-query/v1" {
        return Err(());
    }
    let mut kinds = Vec::new();
    for raw in pairs {
        let Some((raw_name, _)) = raw.split_once('=') else {
            break;
        };
        let Ok(name) = decode_component(raw_name) else {
            break;
        };
        if name != "kinds" {
            break;
        }
        let (_, value) = decode_pair(raw)?;
        kinds.push(value);
    }
    if kinds.is_empty()
        || kinds.len() > 2
        || kinds.windows(2).any(|pair| pair[0] >= pair[1])
        || kinds.iter().any(|kind| kind != "approval" && kind != "run")
    {
        return Err(());
    }
    let mut required = Vec::new();
    if kinds.iter().any(|kind| kind == "approval") {
        required.push(Capability::ApprovalRead);
    }
    if kinds.iter().any(|kind| kind == "run") {
        required.push(Capability::RunRead);
    }
    Ok(required)
}

fn validate_get_target(route: MountedRoute, path: &str, query: Option<&str>) -> Result<(), ()> {
    match route.path {
        "/operator/v1/runs/:run_id"
        | "/operator/v1/approvals/:request_id"
        | "/operator/v1/commands/:command_id" => {
            if query.is_some() {
                return Err(());
            }
            parse_path_uuid(path).map(|_| ())
        }
        "/operator/v1/attention" => validate_page_query(
            query,
            "proof.operator.attention-query/v1",
            &[
                FieldRule::array("kinds", &["approval", "run"], 2),
                FieldRule::array(
                    "states",
                    &["awaiting_decision", "recoverable", "running", "terminal"],
                    4,
                ),
                FieldRule::page_size(),
                FieldRule::cursor(),
            ],
        ),
        "/operator/v1/approvals" => validate_page_query(
            query,
            "proof.operator.approval-query/v1",
            &[
                FieldRule::array("states", &["approved", "denied", "expired", "pending"], 4),
                FieldRule::page_size(),
                FieldRule::cursor(),
            ],
        ),
        "/operator/v1/commands" => validate_page_query(
            query,
            "proof.operator.command-query/v1",
            &[
                FieldRule::array(
                    "kinds",
                    &[
                        "approval_decide",
                        "run_cancel",
                        "run_resume",
                        "session_revoke",
                    ],
                    4,
                ),
                FieldRule::array("outcomes", &["already_terminal", "applied"], 2),
                FieldRule::optional_uuid("run_id"),
                FieldRule::page_size(),
                FieldRule::cursor(),
            ],
        ),
        "/operator/v1/audit" => validate_page_query(
            query,
            "proof.operator.audit-query/v1",
            &[
                FieldRule::array("kinds", AUDIT_KINDS, AUDIT_KINDS.len()),
                FieldRule::optional_uuid("run_id"),
                FieldRule::optional_uuid("approval_request_id"),
                FieldRule::page_size(),
                FieldRule::cursor(),
            ],
        ),
        _ => Err(()),
    }
}

fn decode_command(
    route: MountedRoute,
    path: &str,
    session: &AuthorizedSession,
    body: &[u8],
) -> Result<OperatorCommand, ()> {
    let command: OperatorCommand = strict_decode(body)?;
    let binding = command.binding();
    binding.validate().map_err(|_| ())?;
    if binding.workspace_id != session.workspace_id
        || binding.server_instance_id != session.server_instance_id
        || binding.session_id != session.session_id
        || binding.human_id != session.human_id
        || binding.auth_epoch != session.auth_epoch
        || binding.policy_revision != session.policy_revision
        || binding.session_authority_digest != session.authority_digest
    {
        return Err(());
    }
    let path_id = if route.path == "/operator/v1/session/revoke" {
        None
    } else {
        Some(parse_route_uuid(route, path)?)
    };
    let valid = match (&command, route.path) {
        (OperatorCommand::SessionRevoke(value), "/operator/v1/session/revoke") => {
            value.schema == SessionRevokeRequest::SCHEMA
        }
        (
            OperatorCommand::ApprovalDecision(value),
            "/operator/v1/approvals/:request_id/decisions",
        ) => value.validate().is_ok() && Some(value.approval_request_id) == path_id,
        (OperatorCommand::RunCancel(value), "/operator/v1/runs/:run_id/cancel") => {
            value.validate().is_ok() && Some(value.run_id) == path_id
        }
        (OperatorCommand::RunResume(value), "/operator/v1/runs/:run_id/resume") => {
            value.validate().is_ok() && Some(value.run_id) == path_id
        }
        _ => false,
    };
    valid.then_some(command).ok_or(())
}

fn parse_path_uuid(path: &str) -> Result<Uuid, ()> {
    let segment = path.rsplit('/').next().ok_or(())?;
    if segment.is_empty() || segment.contains('%') {
        return Err(());
    }
    let id = Uuid::parse_str(segment).map_err(|_| ())?;
    if id.get_version_num() != 7
        || id.get_variant() != uuid::Variant::RFC4122
        || id.hyphenated().to_string() != segment
    {
        return Err(());
    }
    Ok(id)
}

fn parse_route_uuid(route: MountedRoute, path: &str) -> Result<Uuid, ()> {
    let mut segments = path.rsplit('/');
    let final_segment = segments.next().ok_or(())?;
    let id = if matches!(
        route.path,
        "/operator/v1/approvals/:request_id/decisions"
            | "/operator/v1/runs/:run_id/cancel"
            | "/operator/v1/runs/:run_id/resume"
    ) {
        segments.next().ok_or(())?
    } else {
        final_segment
    };
    parse_path_uuid(&format!("/{id}"))
}

#[derive(Clone, Copy)]
enum FieldKind {
    Array(&'static [&'static str], usize),
    OptionalUuid,
    PageSize,
    Cursor,
}

#[derive(Clone, Copy)]
struct FieldRule {
    name: &'static str,
    kind: FieldKind,
}

impl FieldRule {
    const fn array(name: &'static str, allowed: &'static [&'static str], maximum: usize) -> Self {
        Self {
            name,
            kind: FieldKind::Array(allowed, maximum),
        }
    }

    const fn optional_uuid(name: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::OptionalUuid,
        }
    }

    const fn page_size() -> Self {
        Self {
            name: "page_size",
            kind: FieldKind::PageSize,
        }
    }

    const fn cursor() -> Self {
        Self {
            name: "cursor",
            kind: FieldKind::Cursor,
        }
    }
}

fn validate_page_query(query: Option<&str>, schema: &str, rules: &[FieldRule]) -> Result<(), ()> {
    let pairs = decode_query(query.ok_or(())?)?;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut observed_order = Vec::new();
    for (name, value) in pairs {
        if observed_order.last() != Some(&name) {
            if groups.contains_key(&name) {
                return Err(());
            }
            observed_order.push(name.clone());
        }
        groups.entry(name).or_default().push(value);
    }
    if observed_order.first().map(String::as_str) != Some("schema") {
        return Err(());
    }
    let expected_order: Vec<_> = std::iter::once("schema")
        .chain(rules.iter().map(|rule| rule.name))
        .collect();
    let mut prior_index = 0;
    for name in observed_order.iter().skip(1) {
        let index = expected_order
            .iter()
            .position(|expected| *expected == name.as_str())
            .ok_or(())?;
        if index <= prior_index {
            return Err(());
        }
        prior_index = index;
    }
    let schema_values = groups.remove("schema").ok_or(())?;
    if schema_values.len() != 1 || schema_values[0] != schema {
        return Err(());
    }
    for rule in rules {
        let values = groups.remove(rule.name);
        match rule.kind {
            FieldKind::Array(allowed, maximum) => {
                let values = values.ok_or(())?;
                if values.is_empty()
                    || values.len() > maximum
                    || values.windows(2).any(|pair| pair[0] >= pair[1])
                    || values
                        .iter()
                        .any(|value| !allowed.contains(&value.as_str()))
                {
                    return Err(());
                }
            }
            FieldKind::OptionalUuid => {
                if let Some(values) = values {
                    if values.len() != 1 || parse_path_uuid(&format!("/{}", values[0])).is_err() {
                        return Err(());
                    }
                }
            }
            FieldKind::PageSize => {
                let values = values.ok_or(())?;
                if values.len() != 1
                    || values[0].starts_with('0')
                    || !values[0]
                        .parse::<u64>()
                        .ok()
                        .is_some_and(|value| (1..=100).contains(&value))
                {
                    return Err(());
                }
            }
            FieldKind::Cursor => {
                if let Some(values) = values {
                    if values.len() != 1
                        || values[0].is_empty()
                        || values[0].len() > 1536
                        || !values[0]
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    {
                        return Err(());
                    }
                }
            }
        }
    }
    if !groups.is_empty() {
        return Err(());
    }
    Ok(())
}

fn decode_query(query: &str) -> Result<Vec<(String, String)>, ()> {
    if query.is_empty() {
        return Err(());
    }
    query.split('&').map(decode_pair).collect()
}

fn decode_pair(pair: &str) -> Result<(String, String), ()> {
    let (name, value) = pair.split_once('=').ok_or(())?;
    if name.is_empty() || value.is_empty() || value.contains('=') {
        return Err(());
    }
    Ok((decode_component(name)?, decode_component(value)?))
}

fn decode_component(component: &str) -> Result<String, ()> {
    if component.contains('+') || !component.is_ascii() {
        return Err(());
    }
    let input = component.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = *input.get(index + 1).ok_or(())?;
            let low = *input.get(index + 2).ok_or(())?;
            if !matches!(high, b'0'..=b'9' | b'A'..=b'F')
                || !matches!(low, b'0'..=b'9' | b'A'..=b'F')
            {
                return Err(());
            }
            let byte = (hex_nibble(high)? << 4) | hex_nibble(low)?;
            if is_unreserved(byte) {
                return Err(());
            }
            output.push(byte);
            index += 3;
        } else {
            if !is_unreserved(input[index]) {
                return Err(());
            }
            output.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| ())
}

fn hex_nibble(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const AUDIT_KINDS: &[&str] = &[
    "approval_decided",
    "approval_expired",
    "budget_committed",
    "budget_forfeited",
    "budget_rejected",
    "budget_released",
    "budget_reserved",
    "command_conflict",
    "command_rejected",
    "control_failure",
    "control_shutdown",
    "dispatch_authorized",
    "lease_acquired",
    "lease_reclaimed",
    "lease_released",
    "lease_renewed",
    "recovery_completed",
    "recovery_started",
    "run_cancelled",
    "run_resumed",
    "runtime_result_committed",
    "session_challenge_issued",
    "session_expired",
    "session_issued",
    "session_replaced",
    "session_revoked",
    "stale_fence_rejected",
];

fn match_route(method: &Method, path: &str) -> Option<MountedRoute> {
    ROUTES.iter().copied().find(|route| {
        method_matches(method, route.method)
            && (route.path == path || pattern_matches(route.path, path))
    })
}

fn method_matches(method: &Method, expected: RouteMethod) -> bool {
    matches!(
        (method, expected),
        (&Method::GET, RouteMethod::Get) | (&Method::POST, RouteMethod::Post)
    )
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<_> = pattern.split('/').collect();
    let path: Vec<_> = path.split('/').collect();
    pattern.len() == path.len()
        && pattern.iter().zip(path).all(|(expected, actual)| {
            if expected.starts_with(':') {
                !actual.is_empty() && !actual.contains('%')
            } else {
                *expected == actual
            }
        })
}

fn single_header_equals(headers: &HeaderMap, name: header::HeaderName, expected: &str) -> bool {
    let values: Vec<_> = headers.get_all(name).iter().collect();
    values.len() == 1 && values[0].as_bytes() == expected.as_bytes()
}

fn session_header_values(headers: &HeaderMap) -> Vec<&[u8]> {
    headers
        .get_all(SESSION_HEADER)
        .iter()
        .map(HeaderValue::as_bytes)
        .collect()
}

enum BodyFailure {
    TooLarge,
}

async fn collect_body(body: Body, limit: usize) -> Result<Zeroizing<Vec<u8>>, BodyFailure> {
    let bytes = to_bytes(body, limit)
        .await
        .map_err(|_| BodyFailure::TooLarge)?;
    Ok(Zeroizing::new(bytes.to_vec()))
}

fn strict_decode<T>(bytes: &[u8]) -> Result<T, ()>
where
    T: for<'de> Deserialize<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let StrictValue(value) = StrictValue::deserialize(&mut deserializer).map_err(|_| ())?;
    deserializer.end().map_err(|_| ())?;
    serde_json::from_value(value).map_err(|_| ())
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a duplicate-free JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate JSON object name"));
            }
            let StrictValue(value) = map.next_value()?;
            values.insert(key, value);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicRate {
    Challenge,
    Exchange,
}

#[derive(Default)]
struct RateState {
    challenge: Option<TokenBucket>,
    exchange: Option<TokenBucket>,
    protected: BTreeMap<Uuid, SessionBucket>,
    access_sequence: u64,
}

struct SessionBucket {
    bucket: TokenBucket,
    last_access: u64,
}

struct TokenBucket {
    capacity_units: u128,
    units: u128,
    refill_per_minute: u128,
    last_tick: u64,
}

impl TokenBucket {
    fn new(capacity: u64, refill_per_minute: u64, tick: u64) -> Self {
        let capacity_units = u128::from(capacity) * RATE_SCALE;
        Self {
            capacity_units,
            units: capacity_units,
            refill_per_minute: u128::from(refill_per_minute),
            last_tick: tick,
        }
    }

    fn consume(&mut self, tick: u64) -> Result<bool, ()> {
        let elapsed = tick.checked_sub(self.last_tick).ok_or(())?;
        self.units = self
            .units
            .saturating_add(u128::from(elapsed) * self.refill_per_minute)
            .min(self.capacity_units);
        self.last_tick = tick;
        if self.units < RATE_SCALE {
            Ok(false)
        } else {
            self.units -= RATE_SCALE;
            Ok(true)
        }
    }
}

fn consume_public_rate(
    state: &OperatorRouterState,
    rate: PublicRate,
    tick: u64,
) -> Result<bool, ()> {
    let Ok(mut rates) = state.rates.lock() else {
        state.fatal_shutdown.store(true, Ordering::SeqCst);
        return Err(());
    };
    let bucket = match rate {
        PublicRate::Challenge => rates
            .challenge
            .get_or_insert_with(|| TokenBucket::new(5, 5, tick)),
        PublicRate::Exchange => rates
            .exchange
            .get_or_insert_with(|| TokenBucket::new(10, 10, tick)),
    };
    bucket.consume(tick)
}

fn consume_protected_rate(
    state: &OperatorRouterState,
    session_id: Uuid,
    tick: u64,
) -> Result<bool, ()> {
    let Ok(mut rates) = state.rates.lock() else {
        state.fatal_shutdown.store(true, Ordering::SeqCst);
        return Err(());
    };
    rates.access_sequence = match rates.access_sequence.checked_add(1) {
        Some(sequence) => sequence,
        None => {
            state.fatal_shutdown.store(true, Ordering::SeqCst);
            return Err(());
        }
    };
    let access_sequence = rates.access_sequence;
    // Two fixed server-instance buckets plus these session buckets keep the
    // complete process inventory at the frozen maximum of 32.
    if !rates.protected.contains_key(&session_id) && rates.protected.len() >= MAX_PROTECTED_BUCKETS
    {
        let evicted = rates
            .protected
            .iter()
            .filter(|(id, _)| **id != session_id)
            .min_by_key(|(id, bucket)| (bucket.last_access, **id))
            .map(|(id, _)| *id);
        if let Some(evicted) = evicted {
            rates.protected.remove(&evicted);
        } else {
            return Err(());
        }
    }
    let session = rates
        .protected
        .entry(session_id)
        .or_insert_with(|| SessionBucket {
            bucket: TokenBucket::new(120, 120, tick),
            last_access: access_sequence,
        });
    session.last_access = access_sequence;
    session.bucket.consume(tick)
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    schema: &'static str,
    code: &'a str,
    message: &'a str,
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .expect("JSON response is valid"),
        Err(_) => unavailable(),
    }
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    json_response(
        status,
        &ErrorEnvelope {
            schema: "proof.operator.error/v1",
            code,
            message,
        },
    )
}

fn not_found() -> Response<Body> {
    error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "The requested resource was not found.",
    )
}

fn invalid_request() -> Response<Body> {
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request is invalid.",
    )
}

fn authentication_required() -> Response<Body> {
    error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "Operator authentication is required.",
    )
}

fn capability_required() -> Response<Body> {
    error_response(
        StatusCode::FORBIDDEN,
        "capability_required",
        "The session lacks the required capability.",
    )
}

fn request_too_large() -> Response<Body> {
    error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request_too_large",
        "The request is too large.",
    )
}

fn unsupported_media_type() -> Response<Body> {
    error_response(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "unsupported_media_type",
        "Content-Type must be application/json.",
    )
}

fn rate_limited() -> Response<Body> {
    error_response(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "The request rate limit was exceeded.",
    )
}

fn unavailable() -> Response<Body> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "control_unavailable",
        "Operator control is unavailable.",
    )
}

fn fatal_unavailable(state: &OperatorRouterState) -> Response<Body> {
    state.fatal_shutdown.store(true, Ordering::SeqCst);
    unavailable()
}

fn harden(response: Response<Body>) -> Response<Body> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .filter(|value| {
            matches!(
                value.as_bytes(),
                b"application/json"
                    | b"text/html; charset=utf-8"
                    | b"text/css; charset=utf-8"
                    | b"application/javascript; charset=utf-8"
            )
        })
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let html = content_type.as_bytes() == b"text/html; charset=utf-8";
    let (_, body) = response.into_parts();
    let mut hardened = Response::new(body);
    *hardened.status_mut() = status;
    let headers = hardened.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    if html {
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'"),
        );
    }
    hardened
}
