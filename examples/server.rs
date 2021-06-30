use benet::{Error, Event, EventKind, Host};
use std::process;
use std::time::{Duration, Instant};

fn run() -> Result<(), Error> {
    const ADDR: &str = "127.0.0.1:8080";

    // Create a server host that will listen on 127.0.0.1:8080, with one channel
    // and a maximum of 32 peers and store the time of connection for each peer.
    let mut host = Host::<Option<Instant>>::builder()
        .addr(ADDR)
        .channel_limit(1)
        .peer_count(32)
        .build()?;

    println!("Listening on {}", ADDR);

    loop {
        // Wait for an event to appear for a maximum duration of one second.
        let Event { mut peer, kind } = match host.service(Duration::from_secs(1))? {
            Some(event) => event,
            None => continue,
        };

        let now = Instant::now();
        let connected = match peer.data() {
            Some(connected) => *connected,
            None => {
                *peer.data_mut() = Some(now);
                now
            }
        };

        let desc = match kind {
            EventKind::Connect(data) => format!("connected (data: {:08X})", data),
            EventKind::Disconnect(data) => format!("disconnected (data: {:08X})", data),
            EventKind::Receive(packet) => {
                format!(
                    "wants to say something: {:?}",
                    String::from_utf8_lossy(packet.data())
                )
            }
        };

        println!(
            "{}, who connected {:?} ago: {}",
            peer.info().addr(),
            now - connected,
            desc
        );
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
        process::exit(1);
    }
}
