use createos::{
    Client, ComputerOpenRequest, ComputerPoint, ComputerScreenId, ComputerScreenOptions,
    ComputerScreenshotOptions, CreateSandboxRequest,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> createos::Result<()> {
    let client = Client::from_env()?;
    let sandbox = client
        .create_sandbox(CreateSandboxRequest {
            shape: "s-2vcpu-4gb".into(),
            rootfs: Some("desktop:1".into()),
            ingress_enabled: true,
            ..Default::default()
        })
        .await?;
    println!("created: {}", sandbox.id());
    let result = async {
        let computer = sandbox.computer();
        let options = ComputerScreenOptions {
            screen_id: Some(ComputerScreenId::SCREEN_0.into()),
            ..Default::default()
        };
        let geometry = loop {
            match computer.screen(options.clone()).await {
                Ok(geometry) => break geometry,
                Err(_) => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        };
        println!("screen: {}x{}", geometry.width, geometry.height);
        let screenshot = computer
            .screenshot(ComputerScreenshotOptions {
                screen: options.clone(),
                ..Default::default()
            })
            .await?
            .bytes()
            .await?;
        println!("screenshot PNG bytes: {}", screenshot.len());
        let point = ComputerPoint { x: 100, y: 100 };
        computer
            .mouse()
            .move_to(point.clone(), options.clone())
            .await?;
        if computer.cursor(options.clone()).await? != point {
            return Err(createos::Error::Protocol("cursor did not move".into()));
        }
        let text = format!("CreateOS desktop {}", sandbox.id());
        computer.set_clipboard(&text, options.clone()).await?;
        if computer.clipboard(options.clone()).await?.text != text {
            return Err(createos::Error::Protocol(
                "clipboard round trip failed".into(),
            ));
        }
        computer
            .open(
                ComputerOpenRequest {
                    target: "https://example.com".into(),
                },
                options,
            )
            .await?;
        let connection = computer
            .screens()
            .connect(&ComputerScreenId::SCREEN_0.into())
            .await?;
        println!("noVNC URL: {}", connection.url);
        Ok::<_, createos::Error>(())
    }
    .await;
    let cleanup = sandbox.destroy().await;
    result?;
    cleanup?;
    println!("destroyed");
    Ok(())
}
