/*
Change the definition of these macros to control the logging level statically
*/

macro_rules! log {
    (@ENDPT, $logger:expr, $($arg:tt)*) => {{
        // if let Some(w) = $logger.line_writer() {
        //     let _ = writeln!(w, $($arg)*);
        // }
    }};
    ($logger:expr, $($arg:tt)*) => {{
        // if let Some(w) = $logger.line_writer() {
        //     let _ = writeln!(w, $($arg)*);
        // }
    }};
}
