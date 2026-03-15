pub(crate) mod io;
pub mod master;
pub mod slave;
pub mod state;

pub use master::MasterConnection;
pub use slave::SlaveConnection;
pub use state::ConnectionState;
