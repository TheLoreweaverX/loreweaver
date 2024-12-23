pub mod mongo;

pub struct Credentials {
    pub conn_url: String,
    pub db: String,
    pub vec_collection: String,
    pub stats_collection: String,
}
