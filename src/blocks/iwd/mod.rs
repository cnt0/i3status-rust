use chan::Sender;
use std::default::Default;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use block::{Block, ConfigBlock};
use config::Config;
use errors::*;
use scheduler::Task;
use widget::{I3BarWidget, State};
use widgets::text::TextWidget;

use self::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged as PropsChanged;
use blocks::dbus::{arg::RefArg, stdintf, BusType, Connection, ConnectionItem, Path, SignalArgs};
use uuid::Uuid;

mod net_connman_iwd_device;
mod net_connman_iwd_network;
use self::net_connman_iwd_device::NetConnmanIwdDevice;
use self::net_connman_iwd_network::NetConnmanIwdNetwork;

const IWD_IFACE: &str = "net.connman.iwd";

const STATE_CONNECTED: &str = "connected";
const STATE_DISCONNECTED: &str = "disconnected";
const STATE_DISCONNECTING: &str = "disconnecting";

const CHANGE_NETWORK: &str = "ConnectedNetwork";
const CHANGE_STATE: &str = "State";

const TIMEOUT: i32 = 100000;

fn get_widget_state(state: &str) -> State {
    match state {
        STATE_DISCONNECTED => State::Critical,
        STATE_CONNECTED => State::Good,
        _ => State::Warning,
    }
}

pub struct IWD {
    id: String,
    network: TextWidget,
    cur_state: Arc<Mutex<IWDPrivate>>,
    dbus_conn: Connection,
}

#[derive(Default, Debug)]
struct IWDPrivate {
    network_obj: String,
    state: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct IWDConfig {
    /// Name of the wifi device to be monitored by this block.
    pub device_id: String,
}

impl IWDConfig {}

impl ConfigBlock for IWD {
    type Config = IWDConfig;

    fn new(block_config: Self::Config, config: Config, send: Sender<Task>) -> Result<Self> {
        let id: String = Uuid::new_v4().simple().to_string();
        let id_copy = id.clone();
        let cur_state: Arc<Mutex<IWDPrivate>> = Arc::new(Mutex::new(Default::default()));
        let cur_state_copy = cur_state.clone();

        thread::spawn(move || {
            let c = Connection::get_private(BusType::System).unwrap();
            let device_id_copy = block_config.device_id.clone();
            {
                let state = &mut *cur_state.lock().unwrap();
                let device = c.with_path(IWD_IFACE, device_id_copy, TIMEOUT);
                state.state = device.get_state().unwrap();
                if state.state == STATE_CONNECTED {
                    state.network_obj = device.get_connected_network().unwrap().to_string();
                }
            }
            c.add_match(&PropsChanged::match_str(
                Some(&IWD_IFACE.into()),
                Some(&Path::from(block_config.device_id)),
            )).unwrap();
            loop {
                for ci in c.iter(TIMEOUT) {
                    if let ConnectionItem::Signal(msg) = ci {
                        if let Some(props) = PropsChanged::from_message(&msg) {
                            let state = &mut *cur_state.lock().unwrap();
                            if let Some(new_obj) = props.changed_properties.get(CHANGE_NETWORK) {
                                state.network_obj = new_obj.as_str().unwrap().to_string();
                            }
                            if let Some(new_state) = props.changed_properties.get(CHANGE_STATE) {
                                state.state = new_state.as_str().unwrap().to_string();
                            }
                            send.send(Task {
                                id: id.clone(),
                                update_time: Instant::now(),
                            });
                        }
                    }
                }
            }
        });

        Ok(IWD {
            id: id_copy,
            cur_state: cur_state_copy,
            network: TextWidget::new(config.clone())
                .with_icon("wifi")
                .with_state(State::Critical)
                .with_text(STATE_DISCONNECTED),
            dbus_conn: Connection::get_private(BusType::System)
                .block_error("iwd", "failed to establish D-Bus connection")?,
        })
    }
}

impl Block for IWD {
    fn id(&self) -> &str {
        &self.id
    }

    fn update(&mut self) -> Result<Option<Duration>> {
        let cur_state = &mut *self.cur_state.lock().unwrap();
        self.network
            .set_state(get_widget_state(cur_state.state.as_str()));
        self.network.set_text(match cur_state.state.as_str() {
            STATE_DISCONNECTED => STATE_DISCONNECTED.to_owned(),
            STATE_DISCONNECTING => STATE_DISCONNECTING.to_owned(),
            _ => NetConnmanIwdNetwork::get_name(&self.dbus_conn.with_path(
                IWD_IFACE,
                cur_state.network_obj.as_str(),
                TIMEOUT,
            )).unwrap(),
        });
        Ok(None)
    }

    fn view(&self) -> Vec<&I3BarWidget> {
        vec![&self.network]
    }
}
