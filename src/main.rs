use std::net::{Ipv4Addr, SocketAddrV4};
use std::thread;
use std::time::Duration;

use config::Config;
use rand::RngCore;

mod tcp;
mod message;

#[derive(Debug, Default, serde_derive::Deserialize, PartialEq, Eq)]
struct AppConfig {
    server: bool,
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::builder()
        .add_source(
            config::Environment::with_prefix("GQ")
                .try_parsing(true)
                .separator("_")
                .list_separator(" "),
        )
        .build()?;

    let app: AppConfig = config.try_deserialize()?;

    if app.server {
        tcp::start_tcp_server(app.port, |data, conn| {
            println!("Successfully received from {}: Handshake # {}", conn.addr(), data[0]);
            if data[0] > 25 {
                return; // 停止
            }
            let mut data = data;
            data[0] += 1;

            // 发回去
            conn.push_deque(data);
        }).await
    } else {
        let dest_socket = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), app.port);
        let conn = tcp::open_tcp_connection(dest_socket.try_into()?, |data, conn| {
            println!("Successfully received from {}: Handshake # {}", conn.addr(), data[0]);
            let mut data = data;
            data[0] += 1;

            // 随机等待一段时间，便于看效果
            let mut rng = rand::thread_rng();
            let sleep_for = rng.next_u64() % 500;
            thread::sleep(Duration::from_millis(sleep_for));

            // 发回去
            conn.push_deque(data);
        }).await?;

        // 试发
        let data = vec![0];
        conn.push_deque(data);

        // 阻塞
        loop {}
    }
}
