use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetConsoleWindow() -> isize;
        fn FreeConsole() -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn PostMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> i32;
    }
    const WM_CLOSE: u32 = 0x0010;

    let Some(pid) = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u32>().ok())
    else {
        eprintln!("usage: close-console <pid>");
        return ExitCode::from(2);
    };
    unsafe {
        if AttachConsole(pid) == 0 {
            eprintln!("cannot attach to the console of process {pid}");
            return ExitCode::FAILURE;
        }
        let window = GetConsoleWindow();
        FreeConsole();
        if window == 0 || PostMessageW(window, WM_CLOSE, 0, 0) == 0 {
            eprintln!("cannot close the console window of process {pid}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("close-console only runs on Windows");
    ExitCode::from(2)
}
