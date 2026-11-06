use diffkin_domain::RepositoryError;
use std::{process::Command, sync::atomic::AtomicBool, time::Duration};

#[cfg(unix)]
pub(super) fn run(
    command: &mut Command,
    prompt: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String, RepositoryError> {
    use nix::{
        fcntl::{fcntl, FcntlArg, OFlag},
        sys::signal::{killpg, Signal},
        unistd::Pid,
    };
    use std::{
        io::{Read, Write},
        os::{fd::AsFd, unix::process::CommandExt},
        process::{Child, Stdio},
        sync::atomic::Ordering,
        time::Instant,
    };
    let io_error = |_| RepositoryError::IoFailure {
        message: "CLI 启动或通信失败，请检查可执行路径和本机登录状态".into(),
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(RepositoryError::Cancelled {
            message: "AI 请求已取消".into(),
        });
    }
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(io_error)?;
    struct Group(Child);
    impl Drop for Group {
        fn drop(&mut self) {
            let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
            let _ = self.0.wait();
        }
    }
    let mut group = Group(child);
    let mut input = group.0.stdin.take();
    let mut stdout = group.0.stdout.take().unwrap();
    let mut stderr = group.0.stderr.take().unwrap();
    for fd in [input.as_ref().unwrap() as &dyn AsFd, &stdout, &stderr] {
        let flags = fcntl(fd, FcntlArg::F_GETFL)
            .map_err(std::io::Error::other)
            .map_err(io_error)?;
        fcntl(
            fd,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map_err(std::io::Error::other)
        .map_err(io_error)?;
    }
    let started = Instant::now();
    let mut sent = 0;
    let mut out = Vec::new();
    let mut stderr_bytes = 0;
    let mut status = None;
    let mut ended = None;
    let mut eof = [false, false];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(RepositoryError::Cancelled {
                message: "AI 请求已取消".into(),
            });
        }
        if started.elapsed() >= timeout {
            return Err(RepositoryError::BudgetExceeded {
                message: "AI 请求超时，请缩小范围后重试".into(),
            });
        }
        if let Some(stdin) = input.as_mut() {
            match stdin.write(&prompt.as_bytes()[sent..]) {
                Ok(n) => sent += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    input = None;
                }
                Err(e) => return Err(io_error(e)),
            }
            if sent == prompt.len() {
                input = None;
            }
        }
        for (index, reader) in [&mut stdout as &mut dyn Read, &mut stderr]
            .into_iter()
            .enumerate()
        {
            for _ in 0..16 {
                let mut bytes = [0; 8192];
                match reader.read(&mut bytes) {
                    Ok(0) => {
                        eof[index] = true;
                        break;
                    }
                    Ok(n) => {
                        if index == 0 {
                            out.extend_from_slice(&bytes[..n]);
                        } else {
                            stderr_bytes += n;
                        }
                        if out.len() > 512 * 1024 || stderr_bytes > 64 * 1024 {
                            return Err(RepositoryError::BudgetExceeded {
                                message: "CLI 输出超出限制，本次请求已停止".into(),
                            });
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(io_error(e)),
                }
            }
        }
        if status.is_none() {
            status = group.0.try_wait().map_err(io_error)?;
            if status.is_some() {
                ended = Some(Instant::now());
                let _ = killpg(Pid::from_raw(group.0.id() as i32), Signal::SIGKILL);
            }
        }
        if status.is_some()
            && (eof == [true, true]
                || ended.is_some_and(|at| at.elapsed() > Duration::from_millis(200)))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !status.is_some_and(|s| s.success()) {
        return Err(RepositoryError::IoFailure {
            message: "CLI 执行失败，请先在终端确认登录状态、模型权限及 CLI 版本".into(),
        });
    }
    String::from_utf8(out).map_err(|_| RepositoryError::IoFailure {
        message: "CLI 返回了非 UTF-8 内容".into(),
    })
}

#[cfg(not(unix))]
pub(super) fn run(
    _command: &mut Command,
    _prompt: &str,
    _cancel: &AtomicBool,
    _timeout: Duration,
) -> Result<String, RepositoryError> {
    Err(RepositoryError::Unsupported {
        message: "本机 AI 调用目前支持 macOS / Linux".into(),
    })
}
