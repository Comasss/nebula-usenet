//! Bus de eventos del motor: lo que antes eran eventos de Tauri (`app.emit`). Quien muestre la
//! interfaz (la app Tauri, el servidor web) se suscribe y reenvía cada evento con su nombre y
//! sus datos, que son los mismos que espera el frontend.
use crate::api::ApiStatus;
use crate::config::Settings;
use crate::engine::{OpenItem, Snapshot, Toast};
use crate::finish::Countdown;
use crate::logger::LogEntry;
use crate::remote::{DeviceView, RemoteStatus, SessionState};
use crate::transfer::Received;
use serde::Serialize;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::sync::broadcast;

/// Un evento del motor. Se serializa como `{"event": "<nombre>", "payload": …}`.
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "kebab-case")]
pub enum Event {
    /// La cola cambió (instantánea completa).
    Queue(Snapshot),
    /// El historial cambió: la interfaz lo vuelve a pedir.
    History,
    Toast(Toast),
    Log(LogEntry),
    /// .nzb o enlaces que hay que abrir en el diálogo de añadir.
    OpenItems(Vec<OpenItem>),
    /// Enlaces a NZB detectados en el portapapeles.
    ClipboardNzb(Vec<String>),
    /// Los ajustes cambiaron desde el motor (apagado rápido, RSS…).
    Settings(Box<Settings>),
    Rss,
    ApiStatus(ApiStatus),
    PowerCountdown(Countdown),
    PowerCancelled,
    TransferReceived(Received),
    TransferClosed,
    RemoteStatus(RemoteStatus),
    RemoteDevices(Vec<DeviceView>),
    RemotePairRequest(Value),
    RemotePairClosed(Value),
    RemotePairCode(Value),
    RemoteState(SessionState),
    RemoteQueue(Value),
    RemoteHistory(Value),
}

impl Event {
    /// Nombre del evento para el frontend ("queue", "remote-state"…).
    pub fn name(&self) -> &'static str {
        match self {
            Event::Queue(_) => "queue",
            Event::History => "history",
            Event::Toast(_) => "toast",
            Event::Log(_) => "log",
            Event::OpenItems(_) => "open-items",
            Event::ClipboardNzb(_) => "clipboard-nzb",
            Event::Settings(_) => "settings",
            Event::Rss => "rss",
            Event::ApiStatus(_) => "api-status",
            Event::PowerCountdown(_) => "power-countdown",
            Event::PowerCancelled => "power-cancelled",
            Event::TransferReceived(_) => "transfer-received",
            Event::TransferClosed => "transfer-closed",
            Event::RemoteStatus(_) => "remote-status",
            Event::RemoteDevices(_) => "remote-devices",
            Event::RemotePairRequest(_) => "remote-pair-request",
            Event::RemotePairClosed(_) => "remote-pair-closed",
            Event::RemotePairCode(_) => "remote-pair-code",
            Event::RemoteState(_) => "remote-state",
            Event::RemoteQueue(_) => "remote-queue",
            Event::RemoteHistory(_) => "remote-history",
        }
    }

    /// Datos del evento como JSON (`null` si no lleva).
    pub fn payload(&self) -> Value {
        match serde_json::to_value(self) {
            Ok(Value::Object(mut m)) => m.remove("payload").unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }
}

static BUS: LazyLock<broadcast::Sender<Event>> = LazyLock::new(|| broadcast::channel(1024).0);

/// Publica un evento. Si nadie escucha, se pierde (como un evento de Tauri sin ventana).
pub fn emit(ev: Event) {
    let _ = BUS.send(ev);
}

/// Recibe los eventos publicados a partir de ahora. Un receptor lento pierde los más antiguos
/// (`RecvError::Lagged`) en lugar de frenar el motor.
pub fn subscribe() -> broadcast::Receiver<Event> {
    BUS.subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_payloads() {
        let mut rx = subscribe();
        emit(Event::History);
        emit(Event::ClipboardNzb(vec!["https://x/a.nzb".into()]));
        let a = rx.try_recv().unwrap();
        assert_eq!(a.name(), "history");
        assert_eq!(a.payload(), Value::Null);
        let b = rx.try_recv().unwrap();
        assert_eq!(b.name(), "clipboard-nzb");
        assert_eq!(b.payload(), serde_json::json!(["https://x/a.nzb"]));
        // El nombre serializado coincide con `name()`.
        assert_eq!(serde_json::to_value(&b).unwrap()["event"], "clipboard-nzb");
    }
}
