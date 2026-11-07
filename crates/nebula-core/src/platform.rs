//! Lo que el motor le pide al programa que lo aloja y depende de la plataforma: notificaciones,
//! bandeja, ventanas, apagado, asociación de .nzb… La app Tauri lo implementa para Windows y
//! Android; sin interfaz propia (servidor, pruebas) basta con `Headless`, que no hace nada.
pub trait Platform: Send + Sync {
    /// Notificación del sistema (fin de una descarga, enlace detectado…).
    fn notify(&self, _title: &str, _body: &str) {}
    /// Notificación de la cuenta atrás de apagado, con un botón Cancelar.
    fn notify_power(&self, _title: &str, _body: &str, _cancel: &str) {}
    /// Estado de la cola: el tooltip de la bandeja o la notificación permanente de Android.
    fn queue_status(&self, _active: usize, _text: String) {}
    /// La cola se pausó o se reanudó (texto del menú de la bandeja).
    fn tray_paused(&self, _paused: bool) {}
    /// Cambiaron los ajustes o la cuenta atrás de apagado (marca del menú de la bandeja).
    fn refresh_power(&self) {}
    /// Muestra la ventana principal.
    fn show_main(&self) {}
    /// Abre o cierra la ventana de la cuenta atrás de apagado.
    fn power_window(&self, _open: bool) {}
    /// Si se puede apagar, suspender o hibernar el equipo.
    fn supports_power(&self) -> bool {
        false
    }
    /// Apaga ("shutdown"), suspende ("sleep") o hiberna ("hibernate").
    fn power_action(&self, _mode: &str) -> Result<(), String> {
        Err(t!("finish.windowsOnly"))
    }
    /// Abrir los .nzb con Nebula.
    fn set_association(&self, _enable: bool) -> Result<(), String> {
        Ok(())
    }
    /// Iniciar Nebula con el sistema.
    fn set_autostart(&self, _enable: bool) -> Result<(), String> {
        Ok(())
    }
}

/// Sin interfaz: servidor, línea de comandos y pruebas.
pub struct Headless;

impl Platform for Headless {}
