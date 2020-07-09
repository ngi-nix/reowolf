macro_rules! endptlog {
    ($logger:expr, $($arg:tt)*) => {{
    	if cfg!(feature = "endpoint_logging") {
	        if let Some(w) = $logger.line_writer() {
	        	let _ = writeln!(w, $($arg)*);
	        }
	    }
    }};
}
macro_rules! log {
    ($logger:expr, $($arg:tt)*) => {{
    	if let Some(w) = $logger.line_writer() {
        	let _ = writeln!(w, $($arg)*);
    	}
    }};
}
