macro_rules! enabled_debug_print {
    (false, $name:literal, $format:literal) => {};
    (false, $name:literal, $format:literal, $($args:expr),*) => {};
    (true, $name:literal, $format:literal) => {
        println!("[{}] {}", $name, $format)
    };
    (true, $name:literal, $format:literal, $($args:expr),*) => {
        println!("[{}] {}", $name, format!($format, $($args),*))
    };
}

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
        #[cfg(not(feature = "no_logging"))]
        if let Some(w) = $logger.line_writer() {
            let _ = writeln!(w, $($arg)*);
        }
    }};
}
