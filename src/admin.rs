use crate::pb::{self, *};
use std::env;
use std::process::{Command, Stdio};
use systemctl;

pub use pb::admin_service_server::AdminServiceServer;

#[derive(Default, Clone)]
pub struct AdminService;

#[tonic::async_trait]
impl pb::admin_service_server::AdminService for AdminService {
    async fn register_service(
        &self,
        request: tonic::Request<RegistryRequest>,
    ) -> std::result::Result<tonic::Response<pb::RegistryResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn start_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn pause_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn resume_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn stop_application(
        &self,
        request: tonic::Request<ApplicationRequest>,
    ) -> std::result::Result<tonic::Response<ApplicationResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn poweroff(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        unimplemented!();
    }
    async fn reboot(
        &self,
        request: tonic::Request<Empty>,
    ) -> std::result::Result<tonic::Response<Empty>, tonic::Status> {
        unimplemented!();
    }

    async fn logs_export(
        &self,
        request: tonic::Request<LogRequest>,
    ) -> std::result::Result<tonic::Response<LogResponse>, tonic::Status> {
        let key = "SYSTEMCTL_PATH";
        let value = "/run/current-system/systemd/bin/systemctl";
        env::set_var(key, value);

        let _async_task = tokio::task::spawn_blocking(|| {
            println!("Starting spawn_blocking");
            let params = request.into_inner();
            let user_name = params.user_name;
            println!("Provided user_name is : {user_name}");
            let remote_server_ip = params.ip;
            println!("Provided ip is : {remote_server_ip}");

            let service_name = String::from("loki.service");

            if let Ok(true) = systemctl::exists(&service_name) {
                if let Ok(true) = systemctl::is_active(&service_name) {
                    println!(
                        "{service_name} is running, going to stop {service_name} now"
                    );
                    let stop_status =
                        systemctl::stop(&service_name).expect("Error in stopping service");
                    if stop_status.success() != true {
                        eprintln!("Cannot stop {service_name} , {stop_status}");
                        let response = LogResponse {
                            cmd_status: format!("Failure!"),
                        };
                        return Ok(tonic::Response::new(response))
                    }

                    let remote = format!("{user_name}@{remote_server_ip}:/tmp");
                    println!("Will start copying Journal logs to {remote}");
                    let copy_status = Command::new("/run/current-system/sw/bin/rsync")
                        .arg("-r") // recursive
                        .arg("-v") // verbose
                        .arg("--progress")
                        .arg("/var/lib/loki") // source of logs
                        .arg(remote) //destination where logs need to be copied
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .status()
                        .expect("Failed to copy logs to remote server");

                    if copy_status.success() {
                        println!("Logs copied to remote server successfully!");
                    } else {
                        eprintln!("Failed to copy logs to remote server, {copy_status}");
                    }

                    let start_status =
                        systemctl::start(&service_name).expect("Error in starting service");
                    if start_status.success() {
                        println!("{service_name} started successfully!");
                    } else {
                        eprintln!("Problem in starting back {service_name} , {start_status}");
                    }
                }
                else{
                    eprintln!("{service_name} not running");
                }
            } else {
                eprintln!("{service_name} does not exist");
            }

            println!("spawn_blocking completed");

            let reply = LogResponse {
                cmd_status: format!("Spawn_blocking Success!"),
            };
            Ok::<tonic::Response<pb::LogResponse>, tonic::Code>(tonic::Response::new(reply))
        })
        .await;

        let response = LogResponse {
            cmd_status: format!("Success!"),
        };
        Ok(tonic::Response::new(response))
    }
}
