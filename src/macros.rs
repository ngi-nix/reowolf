macro_rules! endptlog {
    ($logger:expr, $($arg:tt)*) => {{
    	if cfg!(feature = "endpoint_logging") {
	        let w = $logger.line_writer();
	        let _ = writeln!(w, $($arg)*);
	    }
    }};
}
macro_rules! log {
    ($logger:expr, $($arg:tt)*) => {{
        let _ = writeln!($logger.line_writer(), $($arg)*);
    }};
}
