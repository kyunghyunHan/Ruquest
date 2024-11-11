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

#[derive(Clone, Default, Serialize, Deserialize)]
struct ApiRequest {
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

struct ApiTester {
    requests: Vec<ApiRequest>,
    current_request: ApiRequest,
    methods: Vec<String>,
    tx: Sender<ApiResponse>,
    rx: Receiver<ApiResponse>,
    is_loading: bool,
    runtime: Runtime,
}

impl Default for ApiTester {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            requests: Self::load_requests(),
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
                ui.heading("API Tester");
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
    }
}

impl ApiTester {
    fn load_requests() -> Vec<ApiRequest> {
        if let Ok(data) = fs::read_to_string("saved_requests.json") {
            println!("Loading requests from file"); // 디버그 로그
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            println!("No saved requests file found"); // 디버그 로그
            Vec::new()
        }
    }

    fn save_requests(&self) {
        println!("Saving requests to file"); // 디버그 로그
        if let Ok(json) = serde_json::to_string_pretty(&self.requests) {
            if let Err(e) = fs::write("saved_requests.json", json) {
                println!("Failed to save requests: {}", e); // 디버그 로그
            } else {
                println!("Requests saved successfully"); // 디버그 로그
            }
        }
    }

    fn render_requests_panel(&mut self, ui: &mut Ui) {
        ui.heading("Saved Requests");

        ScrollArea::vertical().show(ui, |ui| {
            let mut to_delete = None;
            for (idx, request) in self.requests.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .button(&format!("{} {}", request.method, request.url))
                        .clicked()
                    {
                        self.current_request = request.clone();
                    }
                    if ui.button("❌").clicked() {
                        to_delete = Some(idx);
                    }
                });
            }

            if let Some(idx) = to_delete {
                self.requests.remove(idx);
                self.save_requests();
            }
        });

        if ui.button("New Request").clicked() {
            self.current_request = ApiRequest::default();
        }
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

            ui.text_edit_singleline(&mut self.current_request.url);

            if ui.button("Send").clicked() && !self.is_loading {
                self.send_request();
            }

            if ui.button("Save").clicked() {
                println!("Save button clicked"); // 디버그 로그
                self.requests.push(self.current_request.clone());
                self.save_requests();
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
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 900.0]),
        ..Default::default()
    };
    let app = ApiTester::default();

    eframe::run_native("API Tester", options, Box::new(|_cc| Ok(Box::new(app))))
}
