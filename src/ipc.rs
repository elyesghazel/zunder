//! Tiny control channel: clients write one command line, the server reads it.
//! A unix socket on Linux and macOS, a localhost port (number kept in a file) on Windows.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::{fs, thread};

#[cfg(unix)]
use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
use std::net::{TcpListener as Listener, TcpStream as Stream};

/// The socket (unix) or the file holding the port (Windows).
pub fn socket_path() -> PathBuf {
    let dir = dirs::runtime_dir().or_else(dirs::data_local_dir).unwrap_or_else(std::env::temp_dir);
    dir.join(if cfg!(windows) { "zunder.port" } else { "zunder.sock" })
}

#[cfg(unix)]
fn connect() -> io::Result<Stream> {
    Stream::connect(socket_path())
}

#[cfg(windows)]
fn connect() -> io::Result<Stream> {
    let port = fs::read_to_string(socket_path())?;
    Stream::connect(("127.0.0.1", port.trim().parse::<u16>().map_err(io::Error::other)?))
}

#[cfg(unix)]
fn bind() -> io::Result<Listener> {
    let path = socket_path();
    let _ = fs::remove_file(&path);
    Listener::bind(&path)
}

#[cfg(windows)]
fn bind() -> io::Result<Listener> {
    let listener = Listener::bind(("127.0.0.1", 0))?;
    let path = socket_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, listener.local_addr()?.port().to_string())?;
    Ok(listener)
}

pub fn send(cmd: &str) -> io::Result<()> {
    connect()?.write_all(cmd.as_bytes())
}

/// Bind and forward every received command into `tx`.
pub fn listen(tx: async_channel::Sender<String>) -> io::Result<()> {
    if connect().is_ok() {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "server already running"));
    }
    let listener = bind()?;
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = String::new();
            if stream.read_to_string(&mut buf).is_ok() && tx.send_blocking(buf.trim_end_matches('\n').to_owned()).is_err() {
                break;
            }
        }
    });
    Ok(())
}
