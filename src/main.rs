use tracing::debug;

mod conf;

use conf::Conf;

#[tokio::main]
async fn main() {
    conf::init_tracing();
    debug!("Tracing initialized");

    let conf = Conf::load();
    debug!("Configuration: {:?}", conf);
}
