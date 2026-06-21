pub struct Server {
    port: u16,
}

impl Server {
    pub fn handle(&self) -> u16 {
        self.port
    }
}

pub fn start(s: Server) -> u16 {
    s.handle()
}
