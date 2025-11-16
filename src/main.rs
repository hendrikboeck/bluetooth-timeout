use tracing::debug;

mod conf;
mod log;

use conf::Conf;

#[tokio::main]
async fn main() {
    log::init_tracing().expect("Could not initialize tracing");
    debug!("Tracing initialized");

    let conf = Conf::load();
    debug!("Configuration: {:?}", conf);
}
