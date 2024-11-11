use eframe::egui;
use egui::{Color32, Context, RichText, ScrollArea, Ui};
use reqwest::{
    header::{HeaderMap, HeaderName},
    Client, Method,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use tokio::runtime::Runtime;

// Request 액션을 위한 enum 추가
#[derive(Clone)]
enum RequestAction {
    Add,
    Select(ApiRequest),
    Delete,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct RequestGroup {
    name: String,
    requests: Vec<ApiRequest>,
    #[serde(skip)]
    is_expanded: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct ApiRequest {
    name: String, // API 별칭
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: String,
    #[serde(skip)]
    response: Option<ApiResponse>,
}

#[derive(Clone)]
struct ApiResponse {
    status: u16,
    headers: HeaderMap,
    body: String,
    time_taken: Duration,
}

#[derive(Default)]
struct NewRequestDialog {
    show: bool,
    name: String,
    group_index: Option<usize>,
}

#[derive(Default)]
struct NewGroupDialog {
    show: bool,
    name: String,
}

struct ApiTester {
    groups: Vec<RequestGroup>,
    current_request: ApiRequest,
    methods: Vec<String>,
    tx: Sender<ApiResponse>,
    rx: Receiver<ApiResponse>,
    is_loading: bool,
    runtime: Runtime,
    new_request_dialog: NewRequestDialog,
    new_group_dialog: NewGroupDialog,
}
impl Default for ApiTester {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            groups: Self::load_groups(),
            current_request: ApiRequest::default(),
            methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
            ],
            tx,
            rx,
            is_loading: false,
            runtime: Runtime::new().expect("Failed to create Tokio runtime"),
            new_request_dialog: NewRequestDialog::default(),
            new_group_dialog: NewGroupDialog::default(),
        }
    }
}

impl ApiTester {
    fn load_groups() -> Vec<RequestGroup> {
        if let Ok(data) = fs::read_to_string("saved_groups.json") {
            println!("Loading groups from file");
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            println!("No saved groups file found");
            Vec::new()
        }
    }

    fn save_groups(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.groups) {
            if let Err(e) = fs::write("saved_groups.json", json) {
                println!("Failed to save groups: {}", e);
            }
        }
    }

    fn render_headers(&mut self, ui: &mut Ui) {
        let mut headers_to_remove = Vec::new();

        for (idx, (key, value)) in self.current_request.headers.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(key);
                ui.text_edit_singleline(value);
                if ui.button("❌").clicked() {
                    headers_to_remove.push(idx);
                }
            });
        }

        for idx in headers_to_remove.iter().rev() {
            self.current_request.headers.remove(*idx);
        }

        if ui.button("Add Header").clicked() {
            self.current_request
                .headers
                .push((String::new(), String::new()));
        }
    }

    fn render_response(&self, ui: &mut Ui, response: &ApiResponse) {
        ui.separator();
        ui.heading("Response");

        ui.horizontal(|ui| {
            let status_color = if response.status < 300 {
                Color32::GREEN
            } else if response.status < 400 {
                Color32::YELLOW
            } else {
                Color32::RED
            };

            ui.label(RichText::new(format!("Status: {}", response.status)).color(status_color));
            ui.label(format!("Time: {:?}", response.time_taken));
        });

        ui.collapsing("Response Headers", |ui| {
            for (key, value) in response.headers.iter() {
                ui.label(format!("{}: {}", key, value.to_str().unwrap_or("")));
            }
        });

        ui.collapsing("Response Body", |ui| {
            if let Ok(json) = serde_json::from_str::<Value>(&response.body) {
                ui.label(serde_json::to_string_pretty(&json).unwrap_or_default());
            } else {
                ui.label(&response.body);
            }
        });
    }
    fn render_requests_panel(&mut self, ui: &mut Ui) {
        ui.heading("API Groups");
    
        if ui.button("New Group").clicked() {
            self.new_group_dialog.show = true;
        }
    
        ScrollArea::vertical().show(ui, |ui| {
            let mut group_to_delete = None;
            let mut request_action = None;  // (group_idx, req_idx, action)
            
            for (group_idx, group) in self.groups.iter_mut().enumerate() {
                // 그룹 헤더
                ui.horizontal(|ui| {
                    if ui.button(if group.is_expanded { "▼" } else { "▶" }).clicked() {
                        group.is_expanded = !group.is_expanded;
                    }
                    ui.label(&group.name);
                    if ui.button("❌").clicked() {
                        group_to_delete = Some(group_idx);
                    }
                });
    
                // 그룹이 확장되어 있을 때 내용 표시
                if group.is_expanded {
                    ui.indent("requests", |ui| {
                        // 새 API 요청 추가 버튼
                        if ui.button("+Add API").clicked() {
                            request_action = Some((group_idx, 0, RequestAction::Add));
                        }
    
                        // API 요청 목록
                        for (req_idx, request) in group.requests.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ui.button(&format!("{} - {}", request.name, request.method)).clicked() {
                                    request_action = Some((group_idx, req_idx, RequestAction::Select(request.clone())));
                                }
                                if ui.button("❌").clicked() {
                                    request_action = Some((group_idx, req_idx, RequestAction::Delete));
                                }
                            });
                        }
                    });
                }
            }
    
            // 액션 처리
            match request_action {
                Some((group_idx, req_idx, RequestAction::Add)) => {
                    self.new_request_dialog.show = true;
                    self.new_request_dialog.group_index = Some(group_idx);
                    self.current_request = ApiRequest::default();
                }
                Some((group_idx, req_idx, RequestAction::Select(request))) => {
                    self.current_request = request;
                    self.new_request_dialog.group_index = Some(group_idx);  // 이 부분이 추가됨
                }
                Some((group_idx, req_idx, RequestAction::Delete)) => {
                    if let Some(group) = self.groups.get_mut(group_idx) {
                        group.requests.remove(req_idx);
                        self.save_groups();
                    }
                }
                None => {}
            }
    
            if let Some(idx) = group_to_delete {
                self.groups.remove(idx);
                self.save_groups();
            }
        });
    }

    fn render_main_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Method")
                .selected_text(&self.current_request.method)
                .show_ui(ui, |ui| {
                    for method in &self.methods {
                        ui.selectable_value(
                            &mut self.current_request.method,
                            method.clone(),
                            method,
                        );
                    }
                });
    
            ui.label("URL:");
            let url_changed = ui.text_edit_singleline(&mut self.current_request.url).changed();
    
            // Command+S나 Ctrl+S로 저장
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
                if let Some(group_idx) = self.new_request_dialog.group_index {
                    if group_idx < self.groups.len() {
                        // 현재 요청 업데이트
                        for request in &mut self.groups[group_idx].requests {
                            if request.name == self.current_request.name {
                                *request = self.current_request.clone();
                                self.save_groups();
                                break;
                            }
                        }
                    }
                }
            }
    
            // URL이 변경되었을 때도 저장
            if url_changed {
                if let Some(group_idx) = self.new_request_dialog.group_index {
                    if group_idx < self.groups.len() {
                        // 현재 요청 업데이트
                        for request in &mut self.groups[group_idx].requests {
                            if request.name == self.current_request.name {
                                *request = self.current_request.clone();
                                self.save_groups();
                                break;
                            }
                        }
                    }
                }
            }
    
            if ui.button("Send").clicked() && !self.is_loading {
                self.send_request();
            }
        });
    
        ui.collapsing("Headers", |ui| {
            self.render_headers(ui);
        });
    
        if self.current_request.method != "GET" {
            ui.collapsing("Body", |ui| {
                ui.text_edit_multiline(&mut self.current_request.body);
            });
        }
    
        if let Some(response) = &self.current_request.response {
            self.render_response(ui, response);
        }
    }
    fn send_request(&mut self) {
        let req = self.current_request.clone();
        let tx = self.tx.clone();
        self.is_loading = true;

        self.runtime.spawn(async move {
            let client = Client::new();
            let method = match req.method.as_str() {
                "GET" => Method::GET,
                "POST" => Method::POST,
                "PUT" => Method::PUT,
                "DELETE" => Method::DELETE,
                "PATCH" => Method::PATCH,
                _ => {
                    let _ = tx.send(ApiResponse {
                        status: 0,
                        headers: HeaderMap::new(),
                        body: format!("Error: Invalid HTTP method '{}'", req.method),
                        time_taken: Duration::from_secs(0),
                    });
                    return;
                }
            };

            let start_time = std::time::Instant::now();
            let mut request = client.request(method, &req.url);

            let mut headers = HeaderMap::new();
            headers.insert(
                HeaderName::from_static("content-type"),
                "application/json".parse().unwrap(),
            );

            for (key, value) in req.headers {
                if !key.is_empty() && !value.is_empty() {
                    if let Ok(header_name) = HeaderName::from_bytes(key.as_bytes()) {
                        if let Ok(header_value) = value.parse() {
                            headers.insert(header_name, header_value);
                        }
                    }
                }
            }
            request = request.headers(headers);

            if !req.body.is_empty() {
                match serde_json::from_str::<Value>(&req.body) {
                    Ok(json) => {
                        request = request.json(&json);
                    }
                    Err(_) => {
                        request = request.body(req.body);
                    }
                }
            }

            match request.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let headers = response.headers().clone();
                    let body = response.text().await.unwrap_or_default();
                    let time_taken = start_time.elapsed();

                    let _ = tx.send(ApiResponse {
                        status,
                        headers,
                        body,
                        time_taken,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ApiResponse {
                        status: 0,
                        headers: HeaderMap::new(),
                        body: format!("Error: {}", e),
                        time_taken: start_time.elapsed(),
                    });
                }
            }
        });
    }
    fn render_dialogs(&mut self, ctx: &Context) {
        // 새 그룹 생성 다이얼로그
        if self.new_group_dialog.show {
            egui::Window::new("New Group")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Group Name: ");
                        ui.text_edit_singleline(&mut self.new_group_dialog.name);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() && !self.new_group_dialog.name.is_empty() {
                            self.groups.push(RequestGroup {
                                name: self.new_group_dialog.name.clone(),
                                requests: Vec::new(),
                                is_expanded: true,
                            });
                            self.save_groups();
                            self.new_group_dialog.name.clear();
                            self.new_group_dialog.show = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_group_dialog.name.clear();
                            self.new_group_dialog.show = false;
                        }
                    });
                });
        }

        // 새 API 요청 생성 다이얼로그
        if self.new_request_dialog.show {
            egui::Window::new("New API Request")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("API Name: ");
                        ui.text_edit_singleline(&mut self.new_request_dialog.name);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() && !self.new_request_dialog.name.is_empty() {
                            if let Some(group_idx) = self.new_request_dialog.group_index {
                                let mut new_request = self.current_request.clone();
                                new_request.name = self.new_request_dialog.name.clone();
                                self.groups[group_idx].requests.push(new_request);
                                self.save_groups();
                            }
                            self.new_request_dialog.name.clear();
                            self.new_request_dialog.group_index = None;
                            self.new_request_dialog.show = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_request_dialog.name.clear();
                            self.new_request_dialog.group_index = None;
                            self.new_request_dialog.show = false;
                        }
                    });
                });
        }
    }
}

impl eframe::App for ApiTester {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if let Ok(response) = self.rx.try_recv() {
            self.current_request.response = Some(response);
            self.is_loading = false;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Ruquest");
            });
        });

        egui::SidePanel::left("requests_panel")
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                self.render_requests_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_main_panel(ui);
        });

        self.render_dialogs(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 900.0]),
        ..Default::default()
    };
    let app = ApiTester::default();

    eframe::run_native("Ruquest", options, Box::new(|_cc| Ok(Box::new(app))))
}