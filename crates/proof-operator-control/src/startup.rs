use std::{future::Future, path::Path, sync::Arc};

use axum::Router;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::VerifyingKey;
use proof_kernel::{
    control_digest_serialized, HumanEnrollment, OperatorAuthorityAuditStore,
    OperatorControlEnvironment, OperatorDirectoryStore, OperatorRandomPurpose,
};
use proof_operator_auth::{AuthPolicy, OperatorAuthAuthority};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    build_operator_router, workspace::trusted_store_open_request, ControlShellError,
    ControllingTerminal, LoopbackListener, OperatorRouteHandler, OperatorRouterState,
    OperatorStoreOpener, OsControllingTerminal, OsOperatorControlEnvironment, ProcessCursorCodec,
    StaticBundle, TerminalCeremonyError,
};

/// A completely checked, still-unpublished control-plane process. Construction
/// is the publication boundary: no router or clean URL is returned unless all
/// startup stages have succeeded in their frozen order.
pub struct PreparedControlPlane<S, T = OsControllingTerminal>
where
    S: OperatorDirectoryStore + OperatorAuthorityAuditStore + Send + Sync + 'static,
    T: ControllingTerminal,
{
    listener: Option<LoopbackListener>,
    router: Router,
    router_state: OperatorRouterState,
    authority: Arc<OperatorAuthAuthority>,
    cursor: Arc<ProcessCursorCodec>,
    store: Arc<S>,
    terminal: T,
    server_instance_id: Uuid,
    clean_url: String,
    origin: String,
}

impl<S, T> PreparedControlPlane<S, T>
where
    S: OperatorDirectoryStore + OperatorAuthorityAuditStore + Send + Sync + 'static,
    T: ControllingTerminal,
{
    pub fn authority(&self) -> Arc<OperatorAuthAuthority> {
        self.authority.clone()
    }

    pub fn cursor(&self) -> Arc<ProcessCursorCodec> {
        self.cursor.clone()
    }

    pub fn store(&self) -> Arc<S> {
        self.store.clone()
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub fn terminal_mut(&mut self) -> &mut T {
        &mut self.terminal
    }

    pub fn server_instance_id(&self) -> Uuid {
        self.server_instance_id
    }

    pub fn clean_url(&self) -> &str {
        &self.clean_url
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Serves on the already-checked listener. The prepared value, including
    /// its store, TTY, session authority, cursor key, and signer-facing state,
    /// remains live until the listener has stopped and drained.
    pub async fn serve_until<F>(mut self, shutdown: F) -> Result<(), ControlShellError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let listener = self
            .listener
            .take()
            .ok_or(ControlShellError::ListenerUnavailable)?;
        let router_state = self.router_state.clone();
        let serve_result = listener
            .serve_until(self.router.clone(), async move {
                tokio::select! {
                    () = shutdown => {},
                    () = router_state.wait_for_fatal_shutdown() => {},
                }
            })
            .await;
        let authority_result = self
            .authority
            .invalidate_for_shutdown()
            .map_err(|_| ControlShellError::ControlUnavailable);
        match (serve_result, authority_result) {
            (Err(error), _) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

/// Runs the released OS-backed preflight. The caller supplies only the frozen
/// workspace selection and W4 assembly seams; address, origin, trust anchor,
/// clock, entropy, and terminal are not configurable.
pub async fn preflight_os_control_plane<O>(
    workspace_root: &Path,
    static_bundle: Arc<dyn StaticBundle>,
    handler: Arc<dyn OperatorRouteHandler>,
    opener: &O,
) -> Result<PreparedControlPlane<O::Store>, ControlShellError>
where
    O: OperatorStoreOpener,
{
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace_root, static_bundle, handler, opener);
        Err(ControlShellError::UnsupportedPlatform)
    }

    #[cfg(target_os = "linux")]
    {
        preflight_control_plane(
            workspace_root,
            static_bundle,
            handler,
            opener,
            || OsControllingTerminal::open(),
            || -> Arc<dyn OperatorControlEnvironment> {
                Arc::new(OsOperatorControlEnvironment::new())
            },
            || LoopbackListener::bind(),
        )
        .await
    }
}

pub(crate) trait StartupTerminal: ControllingTerminal {
    fn verify_restoration(&mut self) -> Result<(), TerminalCeremonyError>;
}

impl StartupTerminal for OsControllingTerminal {
    fn verify_restoration(&mut self) -> Result<(), TerminalCeremonyError> {
        self.verify_echo_restoration()
    }
}

pub(crate) async fn preflight_control_plane<O, T, OpenTerminal, Environment, Bind, BindFuture>(
    workspace_root: &Path,
    static_bundle: Arc<dyn StaticBundle>,
    handler: Arc<dyn OperatorRouteHandler>,
    opener: &O,
    open_terminal: OpenTerminal,
    environment: Environment,
    bind: Bind,
) -> Result<PreparedControlPlane<O::Store, T>, ControlShellError>
where
    O: OperatorStoreOpener,
    T: StartupTerminal,
    OpenTerminal: FnOnce() -> Result<T, TerminalCeremonyError>,
    Environment: FnOnce() -> Arc<dyn OperatorControlEnvironment>,
    Bind: FnOnce() -> BindFuture,
    BindFuture: Future<Output = Result<LoopbackListener, ControlShellError>>,
{
    let request = trusted_store_open_request(workspace_root)?;
    static_bundle.validate()?;

    let mut terminal = open_terminal().map_err(|_| ControlShellError::TerminalUnavailable)?;
    terminal
        .verify_restoration()
        .map_err(|_| ControlShellError::TerminalUnavailable)?;

    let environment = environment();
    environment
        .trusted_utc_now()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    environment
        .monotonic_millis()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    let server_instance_id = environment
        .new_uuid_v7()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    if server_instance_id.get_version_num() != 7
        || server_instance_id.get_variant() != uuid::Variant::RFC4122
    {
        return Err(ControlShellError::ControlUnavailable);
    }
    let mut auth_randomness_probe = Zeroizing::new([0_u8; 32]);
    environment
        .fill_random(
            OperatorRandomPurpose::ChallengeNonce,
            auth_randomness_probe.as_mut(),
        )
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    environment
        .fill_random(
            OperatorRandomPurpose::SessionToken,
            auth_randomness_probe.as_mut(),
        )
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    let cursor = Arc::new(
        ProcessCursorCodec::new(environment.clone())
            .map_err(|_| ControlShellError::ControlUnavailable)?,
    );

    let store = opener.open_existing(&request, environment.clone())?;
    let workspace = store
        .load_operator_workspace()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    workspace
        .validate()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    let enrollment = HumanEnrollment {
        schema: HumanEnrollment::SCHEMA.to_owned(),
        workspace_id: workspace.workspace_id,
        human: workspace.human.clone(),
        capabilities: workspace.capabilities.clone(),
        capability_set_digest: control_digest_serialized(
            "Proof-Operator-Capability-Set-v1",
            &workspace.capabilities,
        )
        .map_err(|_| ControlShellError::ControlUnavailable)?,
        enrolled_at: workspace.created_at,
    };
    enrollment
        .validate()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    let public_key = decode_human_public_key(&workspace.human.public_key)?;

    let listener = bind().await?;
    let endpoint = listener.origin();
    let clean_url = listener.clean_url();
    let origin = endpoint.origin().to_owned();
    let policy = AuthPolicy::from_workspace(
        &workspace,
        &enrollment,
        server_instance_id,
        public_key,
        origin.clone(),
    )
    .map_err(|_| ControlShellError::ControlUnavailable)?;
    let store = Arc::new(store);
    let audit: Arc<dyn OperatorAuthorityAuditStore> = store.clone();
    let authority = Arc::new(
        OperatorAuthAuthority::new(policy, environment.clone(), audit)
            .map_err(|_| ControlShellError::ControlUnavailable)?,
    );
    let state = OperatorRouterState::new(
        endpoint,
        static_bundle,
        handler,
        authority.clone(),
        environment,
    )?;
    let router = build_operator_router(state.clone());

    Ok(PreparedControlPlane {
        listener: Some(listener),
        router,
        router_state: state,
        authority,
        cursor,
        store,
        terminal,
        server_instance_id,
        clean_url,
        origin,
    })
}

fn decode_human_public_key(value: &str) -> Result<VerifyingKey, ControlShellError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(ControlShellError::ControlUnavailable);
    }
    let bytes: [u8; 32] = decoded
        .try_into()
        .map_err(|_| ControlShellError::ControlUnavailable)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| ControlShellError::ControlUnavailable)
}
