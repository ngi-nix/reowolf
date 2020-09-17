/*
Change the definition of these macros to control the logging level statically
*/

macro_rules! log {
    (@BENCH, $logger:expr, $($arg:tt)*) => {{
        // if let Some(w) = $logger.line_writer() {
        //     let _ = writeln!(w, $($arg)*);
        // }
    }};
    (@MARK, $logger:expr, $($arg:tt)*) => {{
        if let Some(w) = $logger.line_writer() {
            let _ = writeln!(w, $($arg)*);
        }
    }};
    (@ENDPT, $logger:expr, $($arg:tt)*) => {{
        // ignore
    }};
    ($logger:expr, $($arg:tt)*) => {{
        // if let Some(w) = $logger.line_writer() {
        //     let _ = writeln!(w, $($arg)*);
        // }
    }};
}
