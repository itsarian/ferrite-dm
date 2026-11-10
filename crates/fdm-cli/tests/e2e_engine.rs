use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use fdm_cli::DownloadEngine;
use fdm_core::{DownloadState, FilterQuery, HashAlgorithm, NewDownloadRequest, StateFilter, hash::sha256_str};
use fdm_ipc::{IpcClient, IpcRequest, IpcResponse, default_socket_path};
use fdm_native_host::{DaemonBridge, dispatch_native_task};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

async fn wait_for_completion(eng: &Arc<DownloadEngine>, id: i64) -> fdm_core::DownloadRecord {
    for _ in 0..80 {
        let rec = eng.get_download(id).unwrap();
        if matches!(
            rec.state,
            DownloadState::Completed | DownloadState::Error | DownloadState::Stopped
        ) {
            return rec;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    eng.get_download(id).unwrap()
}

#[tokio::test]
async fn e2e_http_download_batch_tags_ipc_and_native_host() {
    let tmp = std::env::temp_dir().join(format!("ferrite_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // 1. Start local loopback HTTP range server serving 512 KiB
    let payload: Vec<u8> = (0..(512 * 1024)).map(|i| ((i * 31) % 256) as u8).collect();
    let payload_arc = Arc::new(payload);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn({
        let data = Arc::clone(&payload_arc);
        async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let data = Arc::clone(&data);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let total = data.len();
                    let is_head = req.starts_with("HEAD ");
                    let range_hdr = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split_once(':').map(|x| x.1.trim().to_string()));
                    if let Some(rh) = range_hdr {
                        let spec = rh.trim_start_matches("bytes=");
                        let (s, e) = spec.split_once('-').unwrap_or(("0", ""));
                        let start: usize = s.parse().unwrap_or(0);
                        let end: usize = e.parse().unwrap_or(total - 1).min(total - 1);
                        let slice = &data[start..=end];
                        let hdr = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            slice.len()
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        if !is_head {
                            let _ = sock.write_all(slice).await;
                        }
                    } else {
                        let hdr = format!(
                            "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                        );
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        if !is_head {
                            let _ = sock.write_all(&data).await;
                        }
                    }
                });
            }
        }
    });

    let eng = DownloadEngine::open(tmp.join("data"), true).unwrap();
    let mut cfg = eng.get_config();
    cfg.core.default_download_path = tmp.join("downloads");
    eng.update_config(cfg).unwrap();

    let sup = eng.spawn_supervisor();
    let ipc_srv = eng.start_ipc_listener().await.unwrap();

    // 2. Add HTTP multi-section download via DownloadEngine & verify SHA-256
    let mut req = NewDownloadRequest::from_url(format!("http://{addr}/bundle.bin"));
    req.target_dir = Some(tmp.join("downloads"));
    let added = eng.add_download(req).await.unwrap();
    let done = wait_for_completion(&eng, added.id).await;
    assert_eq!(done.state, DownloadState::Completed);
    assert_eq!(std::fs::read(done.output_path()).unwrap(), *payload_arc);

    let (actual_hash, _) = eng
        .verify_download_hash(added.id, HashAlgorithm::Sha256, None)
        .unwrap();
    assert_eq!(actual_hash.len(), 64);

    // 3. Batch add paused + tag creation & filter
    let tag = eng.create_tag("Linux", "#22C55E").unwrap();
    let batch = eng
        .add_batch_pattern(
            &format!("http://{addr}/part_[1-3].bin"),
            Some(tmp.join("downloads")),
            true,
        )
        .await
        .unwrap();
    assert_eq!(batch.len(), 3);
    eng.assign_tag(batch[0].id, tag.id).unwrap();

    let filtered = eng.list_downloads(&FilterQuery {
        tag_id: Some(tag.id),
        state_filter: StateFilter::Stopped,
        ..Default::default()
    });
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, batch[0].id);

    // 4. Unix socket IPC + wenativehost create_downloads bridge roundtrip
    struct SockBridge {
        sock: std::path::PathBuf,
    }
    impl DaemonBridge for SockBridge {
        fn call<'a>(
            &'a self,
            req: IpcRequest,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<IpcResponse>> + Send + 'a>> {
            Box::pin(async move { IpcClient::new(&self.sock).send(&req).await })
        }
    }
    let bridge = SockBridge {
        sock: default_socket_path(&tmp.join("data")),
    };
    let nh_resp = dispatch_native_task(
        &bridge,
        &json!({
            "id": 99,
            "type": "create_downloads",
            "downloads": [{
                "url": format!("http://{addr}/from-ext.bin"),
                "suggestedName": "from-ext.bin"
            }]
        }),
    )
    .await;
    assert_eq!(nh_resp["error"], "");
    let ext_id = nh_resp["result"]["downloadIds"][0].as_i64().unwrap();
    let ext_done = wait_for_completion(&eng, ext_id).await;
    assert_eq!(ext_done.state, DownloadState::Completed);

    ipc_srv.abort();
    sup.abort();
    srv.abort();
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = sha256_str("ok");
}

#[tokio::test]
async fn e2e_engine_hls_aes128_stream() {
    let tmp = std::env::temp_dir().join(format!("ferrite_e2e_hls_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let key = *b"0123456789abcdef";
    let iv = [0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let plain = b"HLS-ENGINE-SEGMENT-DATA-XYZ-9876543210";
    let mut enc = [0u8; 64];
    let ct_len = Aes128CbcEnc::new((&key).into(), (&iv).into())
        .encrypt_padded_b2b_mut::<Pkcs7>(plain, &mut enc)
        .unwrap()
        .len();
    let ct = Arc::new(enc[..ct_len].to_vec());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn({
        let ct = Arc::clone(&ct);
        async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let ct = Arc::clone(&ct);
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
                    let (ctype, body): (&str, Vec<u8>) = if path == "/stream.m3u8" {
                        let m3u8 = format!(
                            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://{addr}/k.key\"\n#EXTINF:4.0,\nhttp://{addr}/seg1.ts\n#EXT-X-ENDLIST\n"
                        );
                        ("application/vnd.apple.mpegurl", m3u8.into_bytes())
                    } else if path == "/k.key" {
                        ("application/octet-stream", key.to_vec())
                    } else {
                        ("video/mp2t", (*ct).clone())
                    };
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        }
    });

    let eng = DownloadEngine::open(tmp.join("data"), true).unwrap();
    let sup = eng.spawn_supervisor();
    let mut req = NewDownloadRequest::from_url(format!("http://{addr}/stream.m3u8"));
    req.target_dir = Some(tmp.join("downloads"));
    let added = eng.add_download(req).await.unwrap();
    let done = wait_for_completion(&eng, added.id).await;
    assert_eq!(done.state, DownloadState::Completed);
    assert_eq!(std::fs::read(done.output_path()).unwrap(), plain);

    sup.abort();
    srv.abort();
    let _ = std::fs::remove_dir_all(&tmp);
}
