use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use bson::{bson, doc};
use futures_timer::Delay;
use time::PreciseTime;

use super::{
    description::server::{ServerDescription, ServerType},
    state::{server::Server, Topology},
};
use crate::{
    cmap::{Command, Connection},
    error::Result,
    is_master::IsMasterReply,
    options::{ClientOptions, StreamAddress},
    RUNTIME,
};

const DEFAULT_HEARTBEAT_FREQUENCY: Duration = Duration::from_secs(10);

pub(crate) const MIN_HEARTBEAT_FREQUENCY: Duration = Duration::from_millis(500);

pub(super) struct Monitor {
    address: StreamAddress,
    connection: Option<Connection>,
    server: Weak<Server>,
    server_type: ServerType,
    options: ClientOptions,
}

impl Monitor {
    /// Starts a monitoring thread associated with a given Server. A weak reference is used to
    /// ensure that the monitoring thread doesn't keep the server alive after it's been removed
    /// from the topology or the client has been dropped.
    pub(super) fn start(
        address: StreamAddress,
        server: Weak<Server>,
        options: ClientOptions,
    ) -> Result<()> {
        let mut monitor = Self {
            address,
            connection: None,
            server,
            server_type: ServerType::Unknown,
            options,
        };

        RUNTIME.execute(async move {
            monitor.execute().await;
        });

        Ok(())
    }

    async fn execute(&mut self) {
        let heartbeat_frequency = self
            .options
            .heartbeat_freq
            .unwrap_or(DEFAULT_HEARTBEAT_FREQUENCY);

        while let Some(server) = self.server.upgrade() {
            let topology = match server.topology() {
                Some(topology) => topology,
                None => break,
            };

            if self.check_if_topology_changed(server, &topology).await {
                topology.notify_topology_changed();
            }

            Delay::new(MIN_HEARTBEAT_FREQUENCY).await;

            topology
                .wait_for_topology_check_request(heartbeat_frequency - MIN_HEARTBEAT_FREQUENCY)
                .await;
        }
    }

    /// Checks the the server by running an `isMaster` command. If an I/O error occurs, the
    /// connection will replaced with a new one.
    ///
    /// Returns true if the topology has changed and false otherwise.
    async fn check_if_topology_changed(
        &mut self,
        server: Arc<Server>,
        topology: &Topology,
    ) -> bool {
        // Send an isMaster to the server.
        let server_description = self.check_server();
        self.server_type = server_description.server_type;

        topology.update(server_description).await
    }

    fn check_server(&mut self) -> ServerDescription {
        let address = self.address.clone();

        match self.perform_is_master() {
            Ok(reply) => ServerDescription::new(address, Some(Ok(reply))),
            Err(e) => {
                self.clear_connection_pool();

                if self.server_type == ServerType::Unknown {
                    return ServerDescription::new(address, Some(Err(e)));
                }

                ServerDescription::new(address, Some(self.perform_is_master()))
            }
        }
    }

    fn perform_is_master(&mut self) -> Result<IsMasterReply> {
        let connection = self.resolve_connection()?;
        let result = is_master(connection);

        if result
            .as_ref()
            .err()
            .map(|e| e.kind.is_network_error())
            .unwrap_or(false)
        {
            self.connection.take();
        }

        result
    }

    fn resolve_connection(&mut self) -> Result<&mut Connection> {
        if let Some(ref mut connection) = self.connection {
            return Ok(connection);
        }

        let connection = Connection::new_monitoring(
            self.address.clone(),
            self.options.connect_timeout,
            self.options.tls_options(),
        )?;

        // Since the connection was not `Some` above, this will always insert the new connection and
        // return a reference to it.
        Ok(self.connection.get_or_insert(connection))
    }

    fn clear_connection_pool(&self) {
        if let Some(server) = self.server.upgrade() {
            server.clear_connection_pool();
        }
    }
}

fn is_master(connection: &mut Connection) -> Result<IsMasterReply> {
    let command = Command::new_read(
        "isMaster".into(),
        "admin".into(),
        None,
        doc! { "isMaster": 1 },
    );

    let start_time = PreciseTime::now();
    let command_response = connection.send_command(command, None)?;
    let end_time = PreciseTime::now();

    let command_response = command_response.body()?;

    Ok(IsMasterReply {
        command_response,
        round_trip_time: Some(start_time.to(end_time).to_std().unwrap()),
    })
}
