#![forbid(unsafe_code)]
//! Loopback-only security shell for the E0002 operator control plane.

mod ceremony;
mod cursor;
mod environment;
mod error;
mod lifecycle;
mod listener;
mod routing;
mod signer;
mod startup;
mod static_bundle;
mod workspace;

pub use ceremony::{
    complete_challenge_ceremony, ChallengeSigner, ControllingTerminal, OsControllingTerminal,
    TerminalCeremonyError,
};
pub use cursor::ProcessCursorCodec;
pub use environment::OsOperatorControlEnvironment;
pub use error::ControlShellError;
pub use lifecycle::{
    shutdown_control_plane, shutdown_for_signal, wait_for_control_signal, ControlSignal,
    ShutdownCoordinator,
};
pub use listener::{LoopbackListener, LoopbackOrigin};
pub use routing::{
    build_operator_router, frozen_route_inventory, MountedRoute, OperatorRouteHandler,
    OperatorRouterState, ProtectedRequest, RouteMethod, SyntheticEffectSnapshot,
    SyntheticRouteHandler,
};
pub use signer::DescriptorHumanChallengeSigner;
pub use startup::{preflight_os_control_plane, PreparedControlPlane};
pub use static_bundle::{
    EmbeddedStaticBundle, StaticAsset, StaticBundle, StaticManifestEntry, StaticSource,
};
pub use workspace::{
    mandatory_repository_proof_directory, open_authoritative_store, OperatorStoreOpener,
    TrustedStoreOpenRequest,
};

#[cfg(test)]
mod tests;
