// Source RCON as Minecraft speaks it: one TCP connection, auth packet, then
// command packets. Packet = i32 LE length, i32 LE id, i32 LE type, body, \0\0.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: i32 = 3;
const COMMAND: i32 = 2;
/// Minecraft caps a response body at 4096 bytes; the rest of the packet is 10.
const MAX_PACKET: i32 = 4106;

pub struct Rcon {
    stream: TcpStream,
    id: i32,
}

pub async fn connect(addr: &str, password: &str) -> io::Result<Rcon> {
    let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr)).await??;
    let mut rcon = Rcon { stream, id: 0 };
    let (id, _) = rcon.send(AUTH, password).await?;
    if id == -1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "wrong rcon password",
        ));
    }
    Ok(rcon)
}

impl Rcon {
    pub async fn command(&mut self, cmd: &str) -> io::Result<String> {
        Ok(self.send(COMMAND, cmd).await?.1)
    }

    // ponytail: reads one response packet; Minecraft splits replies over 4 KiB
    // into several. Only `minecraft.command` on a chatty command can hit that.
    async fn send(&mut self, kind: i32, body: &str) -> io::Result<(i32, String)> {
        self.id += 1;
        let mut packet = Vec::with_capacity(body.len() + 14);
        packet.extend_from_slice(&((body.len() + 10) as i32).to_le_bytes());
        packet.extend_from_slice(&self.id.to_le_bytes());
        packet.extend_from_slice(&kind.to_le_bytes());
        packet.extend_from_slice(body.as_bytes());
        packet.extend_from_slice(&[0, 0]);
        self.stream.write_all(&packet).await?;

        let len = self.stream.read_i32_le().await?;
        if !(10..=MAX_PACKET).contains(&len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("rcon packet length {len}"),
            ));
        }
        let mut buf = vec![0; len as usize];
        self.stream.read_exact(&mut buf).await?;
        let id = i32::from_le_bytes(buf[0..4].try_into().unwrap());
        let text = String::from_utf8_lossy(&buf[8..buf.len() - 2]).into_owned();
        Ok((id, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Fake server: accepts password "pw", echoes commands back as "ran:<cmd>".
    async fn fake_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    loop {
                        let Ok(len) = s.read_i32_le().await else {
                            return;
                        };
                        let mut buf = vec![0; len as usize];
                        s.read_exact(&mut buf).await.unwrap();
                        let id = i32::from_le_bytes(buf[0..4].try_into().unwrap());
                        let kind = i32::from_le_bytes(buf[4..8].try_into().unwrap());
                        let body = std::str::from_utf8(&buf[8..len as usize - 2]).unwrap();
                        let (id, reply) = match kind {
                            AUTH if body == "pw" => (id, String::new()),
                            AUTH => (-1, String::new()),
                            _ => (id, format!("ran:{body}")),
                        };
                        let mut out = Vec::new();
                        out.extend_from_slice(&((reply.len() + 10) as i32).to_le_bytes());
                        out.extend_from_slice(&id.to_le_bytes());
                        out.extend_from_slice(&(if kind == AUTH { 2i32 } else { 0 }).to_le_bytes());
                        out.extend_from_slice(reply.as_bytes());
                        out.extend_from_slice(&[0, 0]);
                        s.write_all(&out).await.unwrap();
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn auth_then_command_round_trips() {
        let addr = fake_server().await;
        let mut rcon = connect(&addr, "pw").await.unwrap();
        assert_eq!(rcon.command("list").await.unwrap(), "ran:list");
        assert_eq!(rcon.command("say é").await.unwrap(), "ran:say é");
        assert_eq!(
            connect(&addr, "nope").await.err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
