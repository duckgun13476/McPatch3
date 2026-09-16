use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::pin;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

use crate::error::BusinessError;
use crate::global_config::GlobalConfig;
use crate::network::DownloadResult;
use crate::network::UpdatingSource;

/// 代表mcpatch私有更新协议
///
/// ## 数据帧格式
///
/// 私有协议的通信格式为一个个数据帧。
///
/// 每个数据帧由两部分组成：\[`长度部分`]和\[`数据部分`]
///
/// \[`长度部分`\]用来描述后面的数据部分的长度，\[`长度部分`\]本身是固定4字节大小的，小端顺序，无符号
///
/// 紧接着的\[`数据部分`\]是变长的，具体有多长需要读取前面的\[`长度部分`\]就能知道
///
/// 比如发送一个2字节short类型的数据128(0x80)，按小端模式翻译成字节就是：`0x80, 0x0`
///
/// 因为长度为2个字节，所以\[`长度部分`\]就是`[0x2, 0x0, 0x0, 0x0]`（\[`长度部分`\]大小固定为4个字节不变）
///
/// 接着\[`数据部分`\]是`0x80, 0x0`，合起来就是`[0x2, 0x0, 0x0, 0x0], [0x80, 0x0]`一共6个字节，组成这一帧的数据
///
/// ## 通信流程
///
/// ```text
///       客户端                    服务端
/// -------------------------------------------
/// 1.发出文件路径字符串
/// 2.发送文件起始字节
/// 3.发送文件结束字节
///                       4.返回一个状态码
///                       5.如果状态码没问题，就会返回实际的文件内容。如果状态码不正确，就没有后续内容
/// ```
pub struct PrivateProtocol {
    /// 服务器地址
    pub addr: String,

    /// 懒惰加载的socket对象，这样就不用为每个文件请求都打开关闭一次socket连接了
    pub tcp_stream: Arc<Mutex<Option<TcpStream>>>,

    /// 打码的关键字，所有日志里的这个关键字都会被打码。通常用来保护服务器ip或者域名地址不被看到
    mask_keyword: String,

    /// 当前这个更新协议的编号，用来做debug用途
    index: u32,
}

impl PrivateProtocol {
    pub fn new(addr: &str, _config: &GlobalConfig, index: u32) -> Self {
        Self {
            addr: addr.to_owned(),
            tcp_stream: Arc::new(Mutex::new(None)),
            mask_keyword: addr.to_owned(),
            index,
        }
    }
}

#[async_trait]
impl UpdatingSource for PrivateProtocol {
    async fn request(
        &mut self,
        path: &str,
        range: &Range<u64>,
        desc: &str,
        config: &GlobalConfig,
    ) -> DownloadResult {
        let mut stream_lock = self.tcp_stream.clone().lock_owned().await;

        // 懒惰加载
        if stream_lock.is_none() {
            let tcp = TcpStream::connect(&self.addr).await?;
            let std_tcp = tcp.into_std().unwrap();
            std_tcp
                .set_read_timeout(Some(Duration::from_millis(config.private_timeout as u64)))
                .unwrap();
            std_tcp
                .set_write_timeout(Some(Duration::from_millis(config.private_timeout as u64)))
                .unwrap();

            *stream_lock = Some(tokio::net::TcpStream::from_std(std_tcp).unwrap());
        }

        let index = self.index;
        let stream = stream_lock.as_mut().unwrap();

        let len = match request_length(stream, path, range).await {
            Ok(len) => len,
            Err(error) => {
                // A private-protocol connection may be closed while CDN bootstrap files
                // are downloading. Never retry on the same known-dead socket.
                *stream_lock = None;
                return Err(error);
            }
        };

        if len < 0 {
            return Ok(Err(BusinessError::new(format!(
                "私有协议({})接收到的状态码 {} 不正确: {} ({})",
                index, len, path, desc
            ))));
        }

        // 状态码没问题就正常接收文件数据
        let data = receive_partial_data(stream_lock, len as u64).await?;

        Ok(Ok((data.count(), Box::pin(data))))
    }

    fn mask_keyword(&self) -> &str {
        &self.mask_keyword
    }
}

async fn request_length(
    stream: &mut TcpStream,
    path: &str,
    range: &Range<u64>,
) -> std::io::Result<i64> {
    send_data(stream, path.as_bytes()).await?;
    stream.write_all(&range.start.to_le_bytes()).await?;
    stream.write_all(&range.end.to_le_bytes()).await?;
    stream.read_i64_le().await
}

/// 发送一个数据帧
async fn send_data(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    // 先发送4字节的长度信息
    stream.write_u64_le(data.len() as u64).await?;

    // 然后发送实际的数据
    stream.write_all(data).await?;

    Ok(())
}

async fn _receive_data<'a>(
    mut stream: OwnedMutexGuard<Option<TcpStream>>,
) -> std::io::Result<PrivatePartialAsyncRead> {
    let len = stream.as_mut().unwrap().read_u64_le().await?;

    Ok(PrivatePartialAsyncRead::new(stream, len))
}

async fn receive_partial_data<'a>(
    stream: OwnedMutexGuard<Option<TcpStream>>,
    count: u64,
) -> std::io::Result<PrivatePartialAsyncRead> {
    Ok(PrivatePartialAsyncRead::new(stream, count))
}

/// 代表一个限制读取数量的Read
pub struct PrivatePartialAsyncRead(OwnedMutexGuard<Option<TcpStream>>, u64);

impl PrivatePartialAsyncRead {
    pub fn new(tcp: OwnedMutexGuard<Option<TcpStream>>, count: u64) -> Self {
        Self(tcp, count)
    }

    pub fn count(&self) -> u64 {
        self.1
    }
}

impl AsyncRead for PrivatePartialAsyncRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.1 == 0 {
            return std::task::Poll::Ready(Ok(()));
        }

        let limit = buf.remaining().min(self.1 as usize);
        let partial = &mut buf.take(limit);

        let read = self.0.as_mut().unwrap();
        pin!(read);

        match read.poll_read(cx, partial) {
            std::task::Poll::Ready(ready) => {
                let adv = partial.filled().len();

                unsafe {
                    buf.assume_init(adv);
                }
                buf.advance(adv);

                self.1 -= adv as u64;

                match ready {
                    Ok(()) if adv == 0 && self.1 != 0 => {
                        *self.0 = None;
                        self.1 = 0;
                        std::task::Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "private protocol response ended before the advertised length",
                        )))
                    }
                    Err(error) => {
                        *self.0 = None;
                        self.1 = 0;
                        std::task::Poll::Ready(Err(error))
                    }
                    Ok(()) => std::task::Poll::Ready(Ok(())),
                }
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn test_config() -> GlobalConfig {
        GlobalConfig {
            urls: Vec::new(),
            version_file_path: String::new(),
            allow_error: false,
            show_finish_message: false,
            show_changelogs_message: false,
            silent_mode: false,
            window_title: String::new(),
            changelogs_window_title: String::new(),
            base_path: String::new(),
            private_timeout: 1_000,
            http_headers: Vec::new(),
            http_timeout: 1_000,
            http_retries: 1,
            http_ignore_certificate: false,
            run_after_update: String::new(),
            show_console: false,
        }
    }

    async fn read_request(stream: &mut TcpStream) {
        let path_len = stream.read_u64_le().await.unwrap();
        let mut path = vec![0; path_len as usize];
        stream.read_exact(&mut path).await.unwrap();
        stream.read_u64_le().await.unwrap();
        stream.read_u64_le().await.unwrap();
    }

    async fn send_response(stream: &mut TcpStream, body: &[u8]) {
        stream.write_i64_le(body.len() as i64).await.unwrap();
        stream.write_all(body).await.unwrap();
    }

    #[tokio::test]
    async fn discards_idle_closed_connection_before_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            read_request(&mut first).await;
            send_response(&mut first, b"profile").await;
            drop(first);

            let (mut second, _) = listener.accept().await.unwrap();
            read_request(&mut second).await;
            send_response(&mut second, b"index").await;
        });

        let config = test_config();
        let mut protocol = PrivateProtocol::new(&addr.to_string(), &config, 0);

        let (_, mut first_body) = protocol
            .request("ui-profile.json", &(0..0), "profile", &config)
            .await
            .unwrap()
            .unwrap();
        let mut first_text = String::new();
        first_body.read_to_string(&mut first_text).await.unwrap();
        assert_eq!(first_text, "profile");
        drop(first_body);

        let stale_result = protocol
            .request("index.json", &(0..0), "index", &config)
            .await;
        assert!(stale_result.is_err());
        assert!(protocol.tcp_stream.lock().await.is_none());

        let (_, mut retry_body) = protocol
            .request("index.json", &(0..0), "index", &config)
            .await
            .unwrap()
            .unwrap();
        let mut retry_text = String::new();
        retry_body.read_to_string(&mut retry_text).await.unwrap();
        assert_eq!(retry_text, "index");
        server.await.unwrap();
    }
}
