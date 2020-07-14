use super::*;

fn secs_since_unix_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_secs_f64())
        .unwrap_or(0.)
}
impl FileLogger {
    pub fn new(connector_id: ConnectorId, file: std::fs::File) -> Self {
        Self(connector_id, file)
    }
}
impl VecLogger {
    pub fn new(connector_id: ConnectorId) -> Self {
        Self(connector_id, Default::default())
    }
}
/////////////////
impl Logger for DummyLogger {
    fn line_writer(&mut self) -> Option<&mut dyn std::io::Write> {
        None
    }
}

impl Logger for VecLogger {
    fn line_writer(&mut self) -> Option<&mut dyn std::io::Write> {
        let _ = write!(&mut self.1, "CID({}) at {:.6} ", self.0, secs_since_unix_epoch());
        Some(self)
    }
}
impl Logger for FileLogger {
    fn line_writer(&mut self) -> Option<&mut dyn std::io::Write> {
        let _ = write!(&mut self.1, "CID({}) at {:.6} ", self.0, secs_since_unix_epoch());
        Some(&mut self.1)
    }
}
///////////////////
impl Drop for VecLogger {
    fn drop(&mut self) {
        let stdout = std::io::stderr();
        let mut lock = stdout.lock();
        writeln!(lock, "--- DROP LOG DUMP ---").unwrap();
        let _ = std::io::Write::write(&mut lock, self.1.as_slice());
    }
}
impl std::io::Write for VecLogger {
    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, std::io::Error> {
        self.1.extend_from_slice(data);
        Ok(data.len())
    }
}
