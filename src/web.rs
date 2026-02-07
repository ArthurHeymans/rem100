//! Web interface for EM100Pro using egui/eframe
//!
//! This module provides a web-based GUI that mirrors the CLI functionality.

use crate::chips::ChipDesc;
use crate::device::{list_devices, DeviceInfo, Em100, HoldPinState};
use crate::sdram::{read_sdram_with_progress, write_sdram_with_progress};
use egui::{Color32, RichText};
use std::sync::{Arc, Mutex};

/// Application state
#[derive(Default)]
pub struct Em100App {
    /// Connected device
    device: Option<Arc<Mutex<Em100>>>,
    /// Device info
    device_info: Option<DeviceInfo>,
    /// Available devices list
    available_devices: Vec<(u8, u8, String)>,
    /// Selected device index
    selected_device: Option<usize>,
    /// Current emulation state
    is_running: bool,
    /// Hold pin state
    hold_pin_state: HoldPinState,
    /// Selected chip
    selected_chip: Option<ChipDesc>,
    /// Chip search query
    chip_search: String,
    /// Available chips (loaded from embedded data or fetched)
    available_chips: Vec<ChipDesc>,
    /// Chip database version
    chip_db_version: String,
    /// File data to upload to device
    upload_file_data: Option<Vec<u8>>,
    /// Upload filename
    upload_filename: String,
    /// Start address for upload
    start_address: String,
    /// Address mode (3 or 4)
    address_mode: u8,
    /// Data downloaded from device
    download_data: Option<Vec<u8>>,
    /// Operation progress (0.0 - 1.0)
    progress: f32,
    /// Progress message
    progress_message: String,
    /// Status message
    status_message: String,
    /// Status is error
    status_is_error: bool,
    /// Debug info
    debug_info: Option<crate::device::DebugInfo>,
    /// Trace output buffer
    trace_buffer: String,
    /// Current panel
    current_panel: Panel,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Panel {
    #[default]
    Device,
    Memory,
    Trace,
    Firmware,
    Debug,
}

impl Em100App {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Load chip database
        let chip_db = crate::chips::ChipDatabase::load_embedded();
        let available_chips = chip_db.list_chips();
        let chip_db_version = chip_db.version.clone();

        Self {
            address_mode: 3,
            start_address: "0".to_string(),
            available_chips,
            chip_db_version,
            ..Default::default()
        }
    }

    /// Refresh the list of available devices
    fn refresh_devices(&mut self) {
        match list_devices() {
            Ok(devices) => {
                self.available_devices = devices;
                self.set_status("Device list refreshed", false);
            }
            Err(e) => {
                self.set_status(&format!("Failed to list devices: {}", e), true);
            }
        }
    }

    /// Connect to a device
    fn connect_device(&mut self, bus: u8, addr: u8) {
        match Em100::open(Some(bus), Some(addr), None) {
            Ok(em100) => {
                let info = em100.get_info();
                self.is_running = em100.get_state().unwrap_or(false);
                self.hold_pin_state = em100.get_hold_pin_state().unwrap_or(HoldPinState::Float);
                self.device_info = Some(info.clone());
                self.device = Some(Arc::new(Mutex::new(em100)));
                self.set_status(&format!("Connected to {}", info.serial), false);
            }
            Err(e) => {
                self.set_status(&format!("Failed to connect: {}", e), true);
            }
        }
    }

    /// Disconnect from device
    fn disconnect_device(&mut self) {
        self.device = None;
        self.device_info = None;
        self.set_status("Disconnected", false);
    }

    /// Set emulation state
    fn set_emulation_state(&mut self, running: bool) {
        let result = if let Some(ref device) = self.device {
            if let Ok(em100) = device.lock() {
                em100.set_state(running)
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(_) => {
                self.is_running = running;
                self.set_status(
                    if running {
                        "Emulation started"
                    } else {
                        "Emulation stopped"
                    },
                    false,
                );
            }
            Err(e) => {
                self.set_status(&format!("Failed to set state: {}", e), true);
            }
        }
    }

    /// Set hold pin state
    fn set_hold_pin(&mut self, state: HoldPinState) {
        let result = if let Some(ref device) = self.device {
            if let Ok(em100) = device.lock() {
                em100.set_hold_pin_state(state)
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(_) => {
                self.hold_pin_state = state;
                self.set_status(&format!("Hold pin set to {}", state), false);
            }
            Err(e) => {
                self.set_status(&format!("Failed to set hold pin: {}", e), true);
            }
        }
    }

    /// Set chip type
    fn set_chip(&mut self, chip: ChipDesc) {
        let result = if let Some(ref device) = self.device {
            if let Ok(mut em100) = device.lock() {
                // Stop emulation before changing chip type (matches CLI --stop --set pattern)
                let _ = em100.set_state(false);
                let res = em100.set_chip_type(&chip);
                // Auto-enable 4-byte mode for large chips
                if res.is_ok() && chip.size > 16 * 1024 * 1024 {
                    if em100.set_address_mode(4).is_ok() {
                        self.address_mode = 4;
                    }
                }
                res
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(_) => {
                // Emulation was stopped before chip change
                self.is_running = false;
                self.set_status(&format!("Chip set to {} {}", chip.vendor, chip.name), false);
                self.selected_chip = Some(chip);
            }
            Err(e) => {
                self.set_status(&format!("Failed to set chip: {}", e), true);
            }
        }
    }

    /// Upload data to device (write file to SDRAM)
    fn upload_to_device(&mut self) {
        let data = match &self.upload_file_data {
            Some(d) => d.clone(),
            None => return,
        };
        let start_addr = parse_hex(&self.start_address).unwrap_or(0) as u32;

        let result = if let Some(ref device) = self.device {
            if let Ok(em100) = device.lock() {
                // Stop emulation before writing to memory
                let _ = em100.set_state(false);
                self.is_running = false;
                self.progress = 0.0;
                self.progress_message = "Uploading to device...".to_string();
                write_sdram_with_progress(&em100, &data, start_addr, None)
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(_) => {
                self.progress = 1.0;
                self.set_status(
                    "Upload complete. Emulation stopped - press Start to resume.",
                    false,
                );
            }
            Err(e) => {
                self.set_status(&format!("Upload failed: {}", e), true);
            }
        }
    }

    /// Download data from device (read SDRAM to file)
    fn download_from_device(&mut self) {
        let size = self
            .selected_chip
            .as_ref()
            .map(|c| c.size as usize)
            .unwrap_or(0x4000000);

        let result = if let Some(ref device) = self.device {
            if let Ok(em100) = device.lock() {
                self.progress = 0.0;
                self.progress_message = "Downloading from device...".to_string();
                read_sdram_with_progress(&em100, 0, size, None)
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(data) => {
                self.download_data = Some(data);
                self.progress = 1.0;
                self.set_status("Download complete", false);
            }
            Err(e) => {
                self.set_status(&format!("Download failed: {}", e), true);
            }
        }
    }

    /// Refresh debug info
    fn refresh_debug_info(&mut self) {
        let result = if let Some(ref device) = self.device {
            if let Ok(em100) = device.lock() {
                em100.get_debug_info()
            } else {
                return;
            }
        } else {
            return;
        };

        match result {
            Ok(info) => {
                self.debug_info = Some(info);
                self.set_status("Debug info refreshed", false);
            }
            Err(e) => {
                self.set_status(&format!("Failed to get debug info: {}", e), true);
            }
        }
    }

    /// Set status message
    fn set_status(&mut self, message: &str, is_error: bool) {
        self.status_message = message.to_string();
        self.status_is_error = is_error;
    }

    /// Render device panel
    fn device_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Device");
        ui.separator();

        // Device list
        ui.horizontal(|ui| {
            if ui.button("Refresh Devices").clicked() {
                self.refresh_devices();
            }
            if self.device.is_some() {
                if ui.button("Disconnect").clicked() {
                    self.disconnect_device();
                }
            }
        });

        // Collect device info first to avoid borrow issues
        let devices: Vec<_> = self.available_devices.iter().cloned().collect();

        if !devices.is_empty() {
            ui.add_space(8.0);
            ui.label("Available devices:");
            for (i, (bus, addr, serial)) in devices.iter().enumerate() {
                let label = format!("Bus {:03} Device {:03}: {}", bus, addr, serial);
                let is_selected = self.selected_device == Some(i);

                if ui.selectable_label(is_selected, &label).clicked() {
                    self.selected_device = Some(i);
                    self.connect_device(*bus, *addr);
                }
            }
        } else {
            ui.add_space(8.0);
            ui.label("No devices found. Click 'Refresh Devices' to scan.");
        }

        // Device info
        if let Some(ref info) = self.device_info {
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

                    ui.label("Chip DB:");
                    ui.label(&self.chip_db_version);
                    ui.end_row();
                });
        }

        // Control panel
        if self.device.is_some() {
            ui.add_space(16.0);
            ui.separator();
            ui.heading("Control");

            ui.horizontal(|ui| {
                ui.label("Emulation:");
                if ui
                    .add_enabled(!self.is_running, egui::Button::new("Start"))
                    .clicked()
                {
                    self.set_emulation_state(true);
                }
                if ui
                    .add_enabled(self.is_running, egui::Button::new("Stop"))
                    .clicked()
                {
                    self.set_emulation_state(false);
                }

                let status_text = if self.is_running {
                    RichText::new("Running").color(Color32::GREEN)
                } else {
                    RichText::new("Stopped").color(Color32::RED)
                };
                ui.label(status_text);
            });

            ui.add_space(8.0);

            let mut hold_pin_changed = None;
            ui.horizontal(|ui| {
                ui.label("Hold Pin:");
                egui::ComboBox::from_id_salt("hold_pin")
                    .selected_text(format!("{}", self.hold_pin_state))
                    .show_ui(ui, |ui| {
                        let mut current = self.hold_pin_state;
                        if ui
                            .selectable_value(&mut current, HoldPinState::Float, "Float")
                            .clicked()
                        {
                            hold_pin_changed = Some(HoldPinState::Float);
                        }
                        if ui
                            .selectable_value(&mut current, HoldPinState::Low, "Low")
                            .clicked()
                        {
                            hold_pin_changed = Some(HoldPinState::Low);
                        }
                        if ui
                            .selectable_value(&mut current, HoldPinState::Input, "Input")
                            .clicked()
                        {
                            hold_pin_changed = Some(HoldPinState::Input);
                        }
                    });
            });
            if let Some(state) = hold_pin_changed {
                self.set_hold_pin(state);
            }

            ui.add_space(8.0);
            let mut address_mode_changed = None;
            ui.horizontal(|ui| {
                ui.label("Address Mode:");
                if ui
                    .selectable_value(&mut self.address_mode, 3, "3-byte")
                    .clicked()
                {
                    address_mode_changed = Some(3);
                }
                if ui
                    .selectable_value(&mut self.address_mode, 4, "4-byte")
                    .clicked()
                {
                    address_mode_changed = Some(4);
                }
            });
            if let Some(mode) = address_mode_changed {
                if let Some(ref device) = self.device {
                    if let Ok(em100) = device.lock() {
                        let _ = em100.set_address_mode(mode);
                    }
                }
            }

            ui.add_space(8.0);

            // Chip selection
            let mut chip_to_set: Option<ChipDesc> = None;
            ui.horizontal(|ui| {
                ui.label("Chip:");
                let selected_text = if let Some(ref chip) = self.selected_chip {
                    format!("{} {} ({} bytes)", chip.vendor, chip.name, chip.size)
                } else {
                    "None selected".to_string()
                };

                egui::ComboBox::from_id_salt("chip_selector")
                    .width(500.0)
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        // Add search filter
                        ui.text_edit_singleline(&mut self.chip_search);
                        ui.separator();

                        // Filter and display chips
                        let search_lower = self.chip_search.to_lowercase();
                        egui::ScrollArea::vertical()
                            .max_height(500.0)
                            .show(ui, |ui| {
                                for chip in &self.available_chips {
                                    let chip_name = format!("{} {}", chip.vendor, chip.name);
                                    if search_lower.is_empty()
                                        || chip_name.to_lowercase().contains(&search_lower)
                                    {
                                        let is_selected = self
                                            .selected_chip
                                            .as_ref()
                                            .map(|c| c.name == chip.name && c.vendor == chip.vendor)
                                            .unwrap_or(false);
                                        if ui.selectable_label(is_selected, &chip_name).clicked() {
                                            chip_to_set = Some(chip.clone());
                                        }
                                    }
                                }
                            });
                    });
            });

            if let Some(chip) = chip_to_set {
                self.set_chip(chip);
            }
        }
    }

    /// Render memory panel
    fn memory_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Memory Operations");
        ui.separator();

        if self.device.is_none() {
            ui.label("Connect to a device first.");
            return;
        }

        ui.separator();

        // Upload to Device section
        ui.heading("Upload to Device");
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.label(&self.upload_filename);
            #[cfg(all(not(target_arch = "wasm32"), feature = "rfd"))]
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    if let Ok(data) = std::fs::read(&path) {
                        self.upload_filename = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.upload_file_data = Some(data);
                    }
                }
            }
            #[cfg(any(target_arch = "wasm32", not(feature = "rfd")))]
            {
                ui.label("(File dialogs not available - use drag and drop)");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Start Address:");
            ui.text_edit_singleline(&mut self.start_address);
        });

        ui.horizontal(|ui| {
            let can_upload = self.upload_file_data.is_some();
            if ui
                .add_enabled(can_upload, egui::Button::new("Upload"))
                .clicked()
            {
                self.upload_to_device();
            }
        });

        ui.add_space(16.0);
        ui.separator();

        // Download from Device section
        ui.heading("Download from Device");
        ui.horizontal(|ui| {
            if ui.button("Download").clicked() {
                self.download_from_device();
            }
            if let Some(ref data) = self.download_data {
                ui.label(format!("{} bytes", data.len()));
                #[cfg(all(not(target_arch = "wasm32"), feature = "rfd"))]
                if ui.button("Save As...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().save_file() {
                        let _ = std::fs::write(&path, data);
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("(Use Save As in browser)");
                }
            }
        });

        // Progress bar
        if self.progress > 0.0 && self.progress < 1.0 {
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(self.progress).text(&self.progress_message));
        }
    }

    /// Render debug panel
    fn debug_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Debug Information");
        ui.separator();

        if self.device.is_none() {
            ui.label("Connect to a device first.");
            return;
        }

        if ui.button("Refresh").clicked() {
            self.refresh_debug_info();
        }

        if let Some(ref info) = self.debug_info {
            ui.add_space(8.0);
            ui.heading("Voltages");

            egui::Grid::new("voltages_grid")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .show(ui, |ui| {
                    ui.label("1.2V:");
                    ui.label(format!("{}mV", info.voltages.v1_2));
                    ui.end_row();

                    ui.label("3.3V:");
                    ui.label(format!("{}mV", info.voltages.v3_3));
                    ui.end_row();

                    ui.label("5V:");
                    ui.label(format!("{}mV", info.voltages.v5));
                    ui.end_row();

                    ui.label("E_VCC:");
                    ui.label(format!("{}mV", info.voltages.e_vcc));
                    ui.end_row();

                    ui.label("Buffer VCC:");
                    ui.label(format!("{}mV", info.voltages.buffer_vcc));
                    ui.end_row();

                    ui.label("Buffer 3.3V:");
                    ui.label(format!("{}mV", info.voltages.buffer_v3_3));
                    ui.end_row();
                });

            ui.add_space(16.0);
            ui.heading("FPGA Registers");

            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, val) in info.fpga_registers.iter().enumerate() {
                            if i % 8 == 0 {
                                ui.end_row();
                                ui.label(RichText::new(format!("{:04x}:", i * 2)).monospace());
                            }
                            ui.label(RichText::new(format!("{:04x}", val)).monospace());
                        }
                    });
                });
        }
    }

    /// Render trace panel
    fn trace_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("SPI Trace");
        ui.separator();

        if self.device.is_none() {
            ui.label("Connect to a device first.");
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Start Trace").clicked() {
                // TODO: Implement trace mode
                self.set_status("Trace mode not yet implemented for web", true);
            }
            if ui.button("Clear").clicked() {
                self.trace_buffer.clear();
            }
        });

        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.trace_buffer.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
            });
    }

    /// Render firmware panel
    fn firmware_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Firmware");
        ui.separator();

        if self.device.is_none() {
            ui.label("Connect to a device first.");
            return;
        }

        ui.label("Firmware operations are dangerous and may brick your device.");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Dump Firmware").clicked() {
                // TODO: Implement firmware dump
                self.set_status("Firmware dump not yet implemented for web", true);
            }
            if ui.button("Update Firmware").clicked() {
                // TODO: Implement firmware update
                self.set_status("Firmware update not yet implemented for web", true);
            }
        });
    }
}

impl eframe::App for Em100App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top panel with navigation
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("EM100Pro Control");
                ui.separator();

                ui.selectable_value(&mut self.current_panel, Panel::Device, "Device");
                ui.selectable_value(&mut self.current_panel, Panel::Memory, "Memory");
                ui.selectable_value(&mut self.current_panel, Panel::Trace, "Trace");
                ui.selectable_value(&mut self.current_panel, Panel::Firmware, "Firmware");
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
                ui.label(RichText::new(&self.status_message).color(color));
            });
        });

        // Central panel
        egui::CentralPanel::default().show(ctx, |ui| match self.current_panel {
            Panel::Device => self.device_panel(ui),
            Panel::Memory => self.memory_panel(ui),
            Panel::Trace => self.trace_panel(ui),
            Panel::Firmware => self.firmware_panel(ui),
            Panel::Debug => self.debug_panel(ui),
        });
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

/// Run the web application (native)
#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "EM100Pro Control",
        native_options,
        Box::new(|cc| Ok(Box::new(Em100App::new(cc)))),
    )
}
