pub mod agent {
    include!("agent.rs");
}
pub mod google {
    pub mod protobuf {
        include!("google.protobuf.rs");
    }
}
pub mod health {
    include!("health.rs");
}
pub mod oci {
    include!("oci.rs");
}
pub mod types {
    include!("types.rs");
}
