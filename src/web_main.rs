//! Web interface entry point for EM100Pro
//!
//! This binary provides a GUI interface using egui/eframe.
//! It can run as a native desktop app or as a WebAssembly app in the browser.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    env_logger::init();
    rem100::web::run()
}

#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use egui::Color32;
    use rem100::chips::{ChipDatabase, ChipDesc};
    use rem100::web_device::{DeviceInfo, Em100Async, HoldPinState};
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen_futures::spawn_local;

    /// Connection state for async device operations
    #[derive(Default)]
    enum ConnectionState {
        #[default]
        Disconnected,
        Connecting,
        Connected,
        Error(String),
    }

    /// Async operation state
    #[derive(Default)]
    enum AsyncOp {
        #[default]
        Idle,
        InProgress(String),
        Success(String),
        Error(String),
    }

    /// Web app state shared with async tasks
    struct SharedState {
        device: Option<Em100Async>,
        device_info: Option<DeviceInfo>,
        is_running: bool,
        hold_pin_state: HoldPinState,
        connection_state: ConnectionState,
        async_op: AsyncOp,
        progress: f32,
        progress_message: String,
        upload_data: Option<Vec<u8>>,
    }

    impl Default for SharedState {
        fn default() -> Self {
            Self {
                device: None,
                device_info: None,
                is_running: false,
                hold_pin_state: HoldPinState::Float,
                connection_state: ConnectionState::Disconnected,
                async_op: AsyncOp::Idle,
                progress: 0.0,
                progress_message: String::new(),
                upload_data: None,
            }
        }
    }

    /// Web app for EM100Pro control via WebUSB
    pub struct Em100WebApp {
        /// Shared state for async operations
        state: Rc<RefCell<SharedState>>,
        /// Available chips
        available_chips: Vec<ChipDesc>,
        /// Selected chip
        selected_chip: Option<ChipDesc>,
        /// Chip search query
        chip_search: String,
        /// Download data
        download_data: Option<Vec<u8>>,
        /// Download filename
        download_filename: String,
        /// Start address for download
        start_address: String,
        /// Address mode (3 or 4)
        address_mode: u8,
        /// Current panel
        current_panel: Panel,
        /// Status message
        status_message: String,
        /// Status is error
        status_is_error: bool,
    }

    #[derive(Default, PartialEq, Clone, Copy)]
    enum Panel {
        #[default]
        Device,
        Memory,
        Debug,
    }

    impl Em100WebApp {
        pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
            // Load chip database
            let chip_db = ChipDatabase::load_embedded();
            let available_chips = chip_db.chips;

            Self {
                state: Rc::new(RefCell::new(SharedState::default())),
                available_chips,
                selected_chip: None,
                chip_search: String::new(),
                download_data: None,
                download_filename: String::new(),
                start_address: "0".to_string(),
                address_mode: 3,
                current_panel: Panel::Device,
                status_message: "Click 'Connect Device' to connect via WebUSB".to_string(),
                status_is_error: false,
            }
        }

        fn set_status(&mut self, message: &str, is_error: bool) {
            self.status_message = message.to_string();
            self.status_is_error = is_error;
        }

        fn request_device(&mut self) {
            let state = self.state.clone();

            // Mark as connecting
            state.borrow_mut().connection_state = ConnectionState::Connecting;

            spawn_local(async move {
                match Em100Async::request_device().await {
                    Ok(device_info) => match Em100Async::open(device_info).await {
                        Ok(mut device) => {
                            let info = device.get_info();
                            let is_running = device.get_state().await.unwrap_or(false);
                            let hold_pin = device
                                .get_hold_pin_state()
                                .await
                                .unwrap_or(HoldPinState::Float);

                            let mut s = state.borrow_mut();
                            s.device_info = Some(info);
                            s.is_running = is_running;
                            s.hold_pin_state = hold_pin;
                            s.device = Some(device);
                            s.connection_state = ConnectionState::Connected;
                            s.async_op = AsyncOp::Success("Connected successfully".to_string());
                        }
                        Err(e) => {
                            let mut s = state.borrow_mut();
                            s.connection_state =
                                ConnectionState::Error(format!("Failed to open device: {}", e));
                            s.async_op = AsyncOp::Error(format!("Connection failed: {}", e));
                        }
                    },
                    Err(e) => {
                        let mut s = state.borrow_mut();
                        s.connection_state =
                            ConnectionState::Error(format!("No device selected: {}", e));
                        s.async_op = AsyncOp::Error(format!("Device request failed: {}", e));
                    }
                }
            });
        }

        fn disconnect(&mut self) {
            let mut s = self.state.borrow_mut();
            s.device = None;
            s.device_info = None;
            s.connection_state = ConnectionState::Disconnected;
            s.async_op = AsyncOp::Success("Disconnected".to_string());
        }

        fn set_emulation_state(&mut self, running: bool) {
            let state = self.state.clone();
            state.borrow_mut().async_op = AsyncOp::InProgress(
                if running {
                    "Starting emulation..."
                } else {
                    "Stopping emulation..."
                }
                .to_string(),
            );

            spawn_local(async move {
                let result = {
                    let mut s = state.borrow_mut();
                    if let Some(ref mut device) = s.device {
                        Some(device.set_state(running).await)
                    } else {
                        None
                    }
                };

                let mut s = state.borrow_mut();
                match result {
                    Some(Ok(_)) => {
                        s.is_running = running;
                        s.async_op = AsyncOp::Success(
                            if running {
                                "Emulation started"
                            } else {
                                "Emulation stopped"
                            }
                            .to_string(),
                        );
                    }
                    Some(Err(e)) => {
                        s.async_op = AsyncOp::Error(format!("Failed to set state: {}", e));
                    }
                    None => {
                        s.async_op = AsyncOp::Error("No device connected".to_string());
                    }
                }
            });
        }

        fn set_hold_pin(&mut self, hold_state: HoldPinState) {
            let state = self.state.clone();
            state.borrow_mut().async_op = AsyncOp::InProgress("Setting hold pin...".to_string());

            spawn_local(async move {
                let result = {
                    let mut s = state.borrow_mut();
                    if let Some(ref mut device) = s.device {
                        Some(device.set_hold_pin_state(hold_state).await)
                    } else {
                        None
                    }
                };

                let mut s = state.borrow_mut();
                match result {
                    Some(Ok(_)) => {
                        s.hold_pin_state = hold_state;
                        s.async_op = AsyncOp::Success(format!("Hold pin set to {}", hold_state));
                    }
                    Some(Err(e)) => {
                        s.async_op = AsyncOp::Error(format!("Failed to set hold pin: {}", e));
                    }
                    None => {
                        s.async_op = AsyncOp::Error("No device connected".to_string());
                    }
                }
            });
        }

        fn set_chip(&mut self, chip: ChipDesc) {
            let state = self.state.clone();
            let chip_clone = chip.clone();
            state.borrow_mut().async_op =
                AsyncOp::InProgress(format!("Setting chip to {} {}...", chip.vendor, chip.name));

            spawn_local(async move {
                let result = {
                    let mut s = state.borrow_mut();
                    if let Some(ref mut device) = s.device {
                        Some(device.set_chip_type(&chip_clone).await)
                    } else {
                        None
                    }
                };

                let mut s = state.borrow_mut();
                match result {
                    Some(Ok(_)) => {
                        s.async_op = AsyncOp::Success(format!(
                            "Chip set to {} {}",
                            chip_clone.vendor, chip_clone.name
                        ));
                    }
                    Some(Err(e)) => {
                        s.async_op = AsyncOp::Error(format!("Failed to set chip: {}", e));
                    }
                    None => {
                        s.async_op = AsyncOp::Error("No device connected".to_string());
                    }
                }
            });

            self.selected_chip = Some(chip);
        }

        fn download_to_device(&mut self) {
            let data = match &self.download_data {
                Some(d) => d.clone(),
                None => return,
            };

            let start_addr = parse_hex(&self.start_address).unwrap_or(0) as u32;
            let state = self.state.clone();

            {
                let mut s = state.borrow_mut();
                s.progress = 0.0;
                s.progress_message = "Downloading...".to_string();
                s.async_op = AsyncOp::InProgress("Downloading data to device...".to_string());
            }

            spawn_local(async move {
                let result = {
                    let mut s = state.borrow_mut();
                    if let Some(ref mut device) = s.device {
                        Some(device.download(&data, start_addr).await)
                    } else {
                        None
                    }
                };

                let mut s = state.borrow_mut();
                s.progress = 1.0;
                match result {
                    Some(Ok(_)) => {
                        s.async_op = AsyncOp::Success("Download complete".to_string());
                    }
                    Some(Err(e)) => {
                        s.async_op = AsyncOp::Error(format!("Download failed: {}", e));
                    }
                    None => {
                        s.async_op = AsyncOp::Error("No device connected".to_string());
                    }
                }
            });
        }

        fn upload_from_device(&mut self) {
            let size = self
                .selected_chip
                .as_ref()
                .map(|c| c.size as usize)
                .unwrap_or(0x4000000);

            let state = self.state.clone();

            {
                let mut s = state.borrow_mut();
                s.progress = 0.0;
                s.progress_message = "Uploading...".to_string();
                s.async_op = AsyncOp::InProgress("Uploading data from device...".to_string());
            }

            spawn_local(async move {
                let result = {
                    let mut s = state.borrow_mut();
                    if let Some(ref mut device) = s.device {
                        Some(device.upload(0, size).await)
                    } else {
                        None
                    }
                };

                let mut s = state.borrow_mut();
                s.progress = 1.0;
                match result {
                    Some(Ok(data)) => {
                        s.upload_data = Some(data);
                        s.async_op = AsyncOp::Success("Upload complete".to_string());
                    }
                    Some(Err(e)) => {
                        s.async_op = AsyncOp::Error(format!("Upload failed: {}", e));
                    }
                    None => {
                        s.async_op = AsyncOp::Error("No device connected".to_string());
                    }
                }
            });
        }

        /// Render device panel
        fn device_panel(&mut self, ui: &mut egui::Ui) {
            ui.heading("Device");
            ui.separator();

            let state = self.state.borrow();
            let is_connected = matches!(state.connection_state, ConnectionState::Connected);
            let is_connecting = matches!(state.connection_state, ConnectionState::Connecting);
            drop(state);

            // Connect/disconnect buttons
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !is_connected && !is_connecting,
                        egui::Button::new("Connect Device"),
                    )
                    .clicked()
                {
                    self.request_device();
                }
                if ui
                    .add_enabled(is_connected, egui::Button::new("Disconnect"))
                    .clicked()
                {
                    self.disconnect();
                }
                if is_connecting {
                    ui.spinner();
                    ui.label("Connecting...");
                }
            });

            // Connection status
            let state = self.state.borrow();
            match &state.connection_state {
                ConnectionState::Disconnected => {
                    ui.label("No device connected");
                }
                ConnectionState::Connecting => {
                    ui.label("Requesting device access...");
                }
                ConnectionState::Connected => {
                    ui.label(egui::RichText::new("Connected").color(Color32::GREEN));
                }
                ConnectionState::Error(e) => {
                    ui.label(egui::RichText::new(format!("Error: {}", e)).color(Color32::RED));
                }
            }

            // Device info
            if let Some(ref info) = state.device_info {
                ui.add_space(16.0);
                ui.separator();
                ui.heading("Device Information");

                egui::Grid::new("device_info_grid")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Serial:");
                        ui.label(&info.serial);
                        ui.end_row();

                        ui.label("Hardware:");
                        ui.label(format!("{:?}", info.hw_version));
                        ui.end_row();

                        ui.label("MCU Version:");
                        ui.label(&info.mcu_version);
                        ui.end_row();

                        ui.label("FPGA Version:");
                        ui.label(&info.fpga_version);
                        ui.end_row();
                    });
            }

            let is_running = state.is_running;
            let hold_pin_state = state.hold_pin_state;
            drop(state);

            // Control panel
            if is_connected {
                ui.add_space(16.0);
                ui.separator();
                ui.heading("Control");

                ui.horizontal(|ui| {
                    ui.label("Emulation:");
                    if ui
                        .add_enabled(!is_running, egui::Button::new("Start"))
                        .clicked()
                    {
                        self.set_emulation_state(true);
                    }
                    if ui
                        .add_enabled(is_running, egui::Button::new("Stop"))
                        .clicked()
                    {
                        self.set_emulation_state(false);
                    }

                    let status_text = if is_running {
                        egui::RichText::new("Running").color(Color32::GREEN)
                    } else {
                        egui::RichText::new("Stopped").color(Color32::RED)
                    };
                    ui.label(status_text);
                });

                ui.add_space(8.0);

                let mut hold_pin_to_set: Option<HoldPinState> = None;
                ui.horizontal(|ui| {
                    ui.label("Hold Pin:");
                    egui::ComboBox::from_id_salt("hold_pin")
                        .selected_text(format!("{}", hold_pin_state))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(hold_pin_state == HoldPinState::Float, "Float")
                                .clicked()
                            {
                                hold_pin_to_set = Some(HoldPinState::Float);
                            }
                            if ui
                                .selectable_label(hold_pin_state == HoldPinState::Low, "Low")
                                .clicked()
                            {
                                hold_pin_to_set = Some(HoldPinState::Low);
                            }
                            if ui
                                .selectable_label(hold_pin_state == HoldPinState::Input, "Input")
                                .clicked()
                            {
                                hold_pin_to_set = Some(HoldPinState::Input);
                            }
                        });
                });
                if let Some(new_state) = hold_pin_to_set {
                    self.set_hold_pin(new_state);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Address Mode:");
                    if ui
                        .selectable_value(&mut self.address_mode, 3, "3-byte")
                        .clicked()
                    {
                        // TODO: Send to device
                    }
                    if ui
                        .selectable_value(&mut self.address_mode, 4, "4-byte")
                        .clicked()
                    {
                        // TODO: Send to device
                    }
                });
            }
        }

        /// Render memory panel
        fn memory_panel(&mut self, ui: &mut egui::Ui) {
            ui.heading("Memory Operations");
            ui.separator();

            let state = self.state.borrow();
            let is_connected = matches!(state.connection_state, ConnectionState::Connected);
            let progress = state.progress;
            let progress_message = state.progress_message.clone();
            let upload_data_len = state.upload_data.as_ref().map(|d| d.len());
            drop(state);

            if !is_connected {
                ui.label("Connect to a device first.");
                return;
            }

            // Chip selection
            ui.horizontal(|ui| {
                ui.label("Chip:");
                if let Some(ref chip) = self.selected_chip {
                    ui.label(format!(
                        "{} {} ({} bytes)",
                        chip.vendor, chip.name, chip.size
                    ));
                } else {
                    ui.label("None selected");
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut self.chip_search);
            });

            // Chip list (filtered)
            let search_lower = self.chip_search.to_lowercase();
            let filtered_chips: Vec<_> = self
                .available_chips
                .iter()
                .filter(|chip| {
                    let name = format!("{} {}", chip.vendor, chip.name);
                    search_lower.is_empty() || name.to_lowercase().contains(&search_lower)
                })
                .cloned()
                .collect();

            let mut chip_to_set: Option<ChipDesc> = None;
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .show(ui, |ui| {
                    for chip in &filtered_chips {
                        let name = format!("{} {}", chip.vendor, chip.name);
                        let is_selected = self
                            .selected_chip
                            .as_ref()
                            .map(|c| c.name == chip.name)
                            .unwrap_or(false);
                        if ui.selectable_label(is_selected, &name).clicked() {
                            chip_to_set = Some(chip.clone());
                        }
                    }
                });

            if let Some(chip) = chip_to_set {
                self.set_chip(chip);
            }

            ui.add_space(16.0);
            ui.separator();

            // Download section
            ui.heading("Download to Device");
            ui.horizontal(|ui| {
                ui.label("File:");
                ui.label(&self.download_filename);
                ui.label("(Drag & drop file onto window)");
            });

            ui.horizontal(|ui| {
                ui.label("Start Address:");
                ui.text_edit_singleline(&mut self.start_address);
            });

            ui.horizontal(|ui| {
                let can_download = self.download_data.is_some();
                if ui
                    .add_enabled(can_download, egui::Button::new("Download"))
                    .clicked()
                {
                    self.download_to_device();
                }
            });

            ui.add_space(16.0);
            ui.separator();

            // Upload section
            ui.heading("Upload from Device");
            ui.horizontal(|ui| {
                if ui.button("Upload").clicked() {
                    self.upload_from_device();
                }
                if let Some(len) = upload_data_len {
                    ui.label(format!("{} bytes", len));
                    // TODO: Add save button that downloads via JS blob
                }
            });

            // Progress bar
            if progress > 0.0 && progress < 1.0 {
                ui.add_space(8.0);
                ui.add(egui::ProgressBar::new(progress).text(&progress_message));
            }
        }

        /// Render debug panel
        fn debug_panel(&mut self, ui: &mut egui::Ui) {
            ui.heading("Debug Information");
            ui.separator();

            let state = self.state.borrow();
            let is_connected = matches!(state.connection_state, ConnectionState::Connected);
            drop(state);

            if !is_connected {
                ui.label("Connect to a device first.");
                return;
            }

            ui.label("Debug panel - voltage readings and FPGA registers coming soon.");
        }
    }

    impl eframe::App for Em100WebApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // Update status from async operations
            {
                let state = self.state.borrow();
                match &state.async_op {
                    AsyncOp::Idle => {}
                    AsyncOp::InProgress(msg) => {
                        self.status_message = msg.clone();
                        self.status_is_error = false;
                    }
                    AsyncOp::Success(msg) => {
                        self.status_message = msg.clone();
                        self.status_is_error = false;
                    }
                    AsyncOp::Error(msg) => {
                        self.status_message = msg.clone();
                        self.status_is_error = true;
                    }
                }
            }

            // Top panel with navigation
            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("EM100Pro Web Interface");
                    ui.separator();

                    ui.selectable_value(&mut self.current_panel, Panel::Device, "Device");
                    ui.selectable_value(&mut self.current_panel, Panel::Memory, "Memory");
                    ui.selectable_value(&mut self.current_panel, Panel::Debug, "Debug");
                });
            });

            // Bottom panel with status
            egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if self.status_is_error {
                        Color32::RED
                    } else {
                        Color32::GREEN
                    };
                    ui.label(egui::RichText::new(&self.status_message).color(color));
                });
            });

            // Central panel
            egui::CentralPanel::default().show(ctx, |ui| match self.current_panel {
                Panel::Device => self.device_panel(ui),
                Panel::Memory => self.memory_panel(ui),
                Panel::Debug => self.debug_panel(ui),
            });

            // Request repaint while async operations are in progress
            let state = self.state.borrow();
            if matches!(state.async_op, AsyncOp::InProgress(_))
                || matches!(state.connection_state, ConnectionState::Connecting)
            {
                ctx.request_repaint();
            }
        }
    }

    /// Parse hex string (with or without 0x prefix)
    fn parse_hex(s: &str) -> Option<u64> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    // Redirect panic messages to the console
    console_error_panic_hook::set_once();

    // Redirect tracing to the console
    tracing_wasm::set_as_global_default();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document")
            .get_element_by_id("em100_canvas")
            .expect("no canvas element")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("not a canvas element");

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(wasm_app::Em100WebApp::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
