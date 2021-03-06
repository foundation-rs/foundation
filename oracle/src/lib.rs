mod environment;
mod oci;
mod connection;
mod types;
mod values;
mod statement;
mod implementors;

pub use oci::{OracleError, OracleResult};
pub use connection::{Connection, SessionPool, create_pool};

pub use types::{
    SqlType,
    SqlDate, SqlDateTime,
    TypeDescriptor,
    TypeDescriptorProducer
};

pub use statement::{
    Statement,
    Query,
    QueryIterator,
    ResultsProvider,
    ResultSet,
    ParamsProvider,
    ParamsProjection,
    ParamValue,
    SQLParams,
    SQLResults
};

pub use statement::params::{
    Identifier,
    Member,
    ValueProjector
};

pub use implementors::GeneralMetaProvider;