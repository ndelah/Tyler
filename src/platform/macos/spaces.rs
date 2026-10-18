//! Native desktop control through yabai's local IPC socket (no shell or per-tick processes).
//! Protocol: native-endian i32 byte count, NUL-separated arguments, final NUL.
//! See asmvik/yabai src/yabai.c:client_send_message.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use core_graphics::display::CGDisplay;
use serde::de::DeserializeOwned;

use crate::spaces::{adjacent_display, Display, Snapshot, Space};
use crate::WindowId;

const LIMIT: u64 = 8 * 1024 * 1024;

pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new() -> Result<Self, String> {
        let user = std::env::var("USER").map_err(|_| "USER is not set".to_string())?;
        if user.is_empty() || user.contains('/') || user.contains('\0') {
            return Err("invalid USER for yabai socket".into());
        }
        Ok(Self {
            socket: PathBuf::from(format!("/tmp/yabai_{user}.socket")),
        })
    }

    fn request(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let mut body = Vec::new();
        for arg in args {
            if arg.contains('\0') {
                return Err("invalid IPC argument".into());
            }
            body.extend_from_slice(arg.as_bytes());
            body.push(0);
        }
        body.push(0);
        let mut stream = UnixStream::connect(&self.socket)
            .map_err(|e| format!("Cannot connect to yabai ({}): {e}. Install yabai, grant it Accessibility access, set its layout to float, and start its service. See docs/tutorial.md.", self.socket.display()))?;
        let timeout = Some(Duration::from_millis(200));
        let mut exchange = || -> std::io::Result<Vec<u8>> {
            stream.set_read_timeout(timeout)?;
            stream.set_write_timeout(timeout)?;
            stream.write_all(&(body.len() as i32).to_ne_bytes())?;
            stream.write_all(&body)?;
            stream.shutdown(Shutdown::Write)?;
            let mut response = Vec::new();
            (&mut stream).take(LIMIT + 1).read_to_end(&mut response)?;
            Ok(response)
        };
        let response = exchange().map_err(|e| format!("yabai IPC failed: {e}"))?;
        if response.len() as u64 > LIMIT {
            return Err("yabai response exceeded 8 MiB".into());
        }
        // yabai prefixes failed commands with ASCII BEL, including empty command replies.
        if response.contains(&7) {
            return Err(String::from_utf8_lossy(&response)
                .trim_start_matches('\x07')
                .trim()
                .to_string());
        }
        Ok(response)
    }

    fn query<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T, String> {
        serde_json::from_slice(&self.request(args)?)
            .map_err(|e| format!("Invalid yabai query response: {e}"))
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let displays: Vec<Display> = self.query(&["query", "--displays"])?;
        let display = displays
            .iter()
            .find(|d| d.id == CGDisplay::main().id)
            .ok_or("yabai did not report the main display")?
            .index;
        let spaces: Vec<Space> = self.query(&["query", "--spaces"])?;
        if !spaces.iter().any(|s| s.display == display) {
            return Err("yabai did not report Spaces for the main display".into());
        }
        Ok(Snapshot { spaces, display })
    }

    fn selector(&self, id: u64) -> Result<String, String> {
        let snapshot = self.snapshot()?;
        snapshot
            .desktops()
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.index.to_string())
            .ok_or_else(|| "That desktop no longer exists on the main display".into())
    }

    pub fn focus(&self, space: u64) -> Result<(), String> {
        let index = self.selector(space)?;
        self.request(&["space", "--focus", &index]).map(|_| ())
    }

    pub fn focus_display(&self, forward: bool) -> Result<(), String> {
        let displays: Vec<Display> = self.query(&["query", "--displays"])?;
        let focused: Display = self.query(&["query", "--displays", "--display"])?;
        let target = adjacent_display(&displays, focused.index, forward)
            .ok_or("The focused display is no longer connected")?;
        if target != focused.index {
            self.request(&["display", "--focus", &target.to_string()])?;
        }
        Ok(())
    }

    pub fn move_window(&self, window: WindowId, space: u64) -> Result<(), String> {
        let index = self.selector(space)?;
        self.request(&["window", &window.to_string(), "--space", &index])
            .map(|_| ())
    }
}

pub fn check() -> Result<(), String> {
    let snapshot = Client::new()?.snapshot()?;
    if snapshot.layout_conflict() {
        return Err("Set yabai's layout to float on the main display; Tyler owns tiling. See docs/tutorial.md.".into());
    }
    println!(
        "Native Spaces ready: {} desktop(s) on the main display; active Space {:?}",
        snapshot.desktops().len(),
        snapshot.active()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn fake(reply: &[u8], args: &[&str]) -> Result<Vec<u8>, String> {
        let dir = std::path::PathBuf::from("/tmp").join(format!(
            "tyler-ipc-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s");
        let listener = UnixListener::bind(&socket).unwrap();
        let reply = reply.to_vec();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut body = Vec::new();
            stream.read_to_end(&mut body).unwrap();
            assert_eq!(body.len(), i32::from_ne_bytes(size) as usize);
            assert_eq!(body, b"space\0--focus\x002\0\0");
            stream.write_all(&reply).unwrap();
        });
        let result = Client {
            socket: socket.clone(),
        }
        .request(args);
        server.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        result
    }

    #[test]
    fn ipc_frames_arguments_and_distinguishes_command_errors_from_empty_success() {
        let args = ["space", "--focus", "2"];
        assert_eq!(fake(b"", &args).unwrap(), b"");
        assert_eq!(
            fake(b"\x07could not focus space\n", &args).unwrap_err(),
            "could not focus space"
        );
    }
}
