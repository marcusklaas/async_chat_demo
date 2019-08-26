use std::{
    net::ToSocketAddrs,
    sync::Arc,
    collections::{
        HashMap,
        hash_map::Entry,
    },
};

use async_std::{
    prelude::*,
    task,
    net::{
        TcpListener,
        TcpStream,
    },
    io::BufReader,
};

use futures::{
    channel::mpsc,
    SinkExt,
    select,
    FutureExt,
};

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
enum Void {}

#[derive(Debug)]
enum Event {
    NewPeer {
        name: String,
        stream: Arc<TcpStream>,
        shutdown: Receiver<Void>,
    },
    Message {
        from: String,
        to: Vec<String>,
        msg: String,
    },
}

async fn broker(mut events: Receiver<Event>) -> Result<()> {
    let (disconnect_sender, mut disconnect_receiver) =
        mpsc::unbounded::<(String, Receiver<String>)>();
    let mut peers: HashMap<String, Sender<String>> = Default::default();

    loop {
        let event = select! {
            event = events.next().fuse() => match event {
                None => break,
                Some(event) => event,
            },
            disconnect = disconnect_receiver.next().fuse() => {
                let (name, _pending_messages) = disconnect.unwrap();
                assert!(peers.remove(&name).is_some());
                continue;
            }
        };
        
        match event {
            Event::Message { from, to, msg } => {
                for addr in to {
                    if let Some(peer) = peers.get_mut(&addr) {
                        peer.send(format!("from {}: {}\n", from, msg)).await?;
                    }
                }
            }
            Event::NewPeer { name, stream, shutdown } => {
                match peers.entry(name.clone()) {
                    Entry::Occupied(..) => {}
                    Entry::Vacant(entry) => {
                        let (client_sender, mut client_receiver) = mpsc::unbounded();
                        entry.insert(client_sender);
                        let mut disconnect_sender = disconnect_sender.clone();
                        spawn_and_log_error(async move {
                            let res = client_writer(&mut client_receiver, stream, shutdown).await;
                            println!("{} disconnected", name.clone());
                            disconnect_sender.send((name, client_receiver)).await.unwrap();
                            res
                        });
                    }
                }
            }
        }
    }
    drop(peers);
    drop(disconnect_sender);
    while let Some(..) = disconnect_receiver.next().await {}
    Ok(())
}

async fn client_writer(
    messages: &mut Receiver<String>,
    stream: Arc<TcpStream>,
    mut shutdown: Receiver<Void>,
) -> Result<()> {
    let mut stream = &*stream;
    loop {
        select! {
            msg = messages.next().fuse() => match msg {
                Some(msg) => stream.write_all(msg.as_bytes()).await?,
                None => break,
            },
            void = shutdown.next().fuse() => match void {
                Some(void) => match void {},
                None => break,
            }
        }
    }
    Ok(())
}

async fn server(addr: impl ToSocketAddrs) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;

    let (broker_sender, broker_receiver) = mpsc::unbounded();
    let broker_handle = task::spawn(broker(broker_receiver));
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        println!("Accepting from: {}", stream.peer_addr()?);
        let _handle = spawn_and_log_error(client(broker_sender.clone(), stream));
    }
    drop(broker_sender);
    broker_handle.await?;
    Ok(())
}

async fn client(mut broker: Sender<Event>, stream: TcpStream) -> Result<()> {
    let stream = Arc::new(stream);
    let mut lines = BufReader::new(&*stream).lines();

    let name = match lines.next().await {
        None => Err("peer disconnected immediately")?,
        Some(line) => line?,
    };
    let (_shutdown_sender, shutdown_receiver) = mpsc::unbounded();
    broker.send(Event::NewPeer {
        name: name.clone(),
        stream: Arc::clone(&stream),
        shutdown: shutdown_receiver,
    }).await.unwrap();
    println!("Name = {}", name);

    while let Some(line) = lines.next().await {
        let line = line?;
        let (dest, msg) = match line.find(':') {
            None => continue,
            Some(idx) => (&line[..idx], line[(idx + 1)..].trim()),
        };
        let dest: Vec<_> = dest.split(',').map(|name| name.trim().to_owned()).collect();
        let msg = msg.trim().to_string();

        broker.send(Event::Message {
            from: name.clone(),
            to: dest,
            msg,
        }).await.unwrap();
    }

    Ok(())
}

fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = fut.await {
            eprintln!("Error: {}", e);
        }
    })
}

fn main() -> Result<()> {
    let fut = server("127.0.0.1:8080");
    task::block_on(fut)
}
