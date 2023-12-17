//! TCP Helper
//!
//! 定义了一组用于进行TCP通信的工具。

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, RwLock};

use lazy_static::lazy_static;
use tokio::io;
use tokio::io::Interest;
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct TcpConnection {
    stream: TcpStream,
    destination: SocketAddr,
    send_deque: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

type TcpConnectionSet = Vec<Arc<TcpConnection>>;

lazy_static! {
    pub static ref SELF_SCOKET_ADDR: RwLock<SocketAddrV4> = RwLock::new(SocketAddrV4::new(Ipv4Addr::new(127,0,0,1), 0));
    pub static ref TCP_CONNECTIONS: RwLock<TcpConnectionSet> = RwLock::new(Vec::new());
}

impl TcpConnection {
    pub fn new(tcp_stream: TcpStream, destination: SocketAddr) -> Self {
        Self {
            stream: tcp_stream,
            destination,
            send_deque: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn addr(self: &Self) -> SocketAddr {
        self.destination
    }

    pub fn start(self: Arc<Self>, callback_fn: fn(Vec<u8>, Arc<Self>)) {
        tokio::spawn(async move {
            let mut read_buffer = Vec::new();
            let mut read_flag = false;
            let mut last_read_num = 0;

            let mut write_buffer = Vec::new();
            let mut write_flag = false;
            let mut write_pos = 0;
            loop {
                let ready = self.stream.ready(Interest::READABLE | Interest::WRITABLE | Interest::ERROR).await.unwrap();

                if ready.is_writable() {
                    // 可以写
                    let write_list_lock = self.send_deque.clone();
                    let write_list = write_list_lock.try_lock();
                    if write_list.is_ok(){
                        let mut write_list = write_list.unwrap();
                        if write_list.len()>0{

                            let buf = write_list.get(0).unwrap();
                            let write_end = usize::min(write_pos + 1024, buf.len());
                            let mut buf_ref = &buf[write_pos..write_end];

                            let mut header_length = 0usize;

                            if !write_flag{
                                let small_buf = (buf.len() as u64).to_be_bytes();

                                write_buffer.clear();
                                write_buffer.extend_from_slice(&small_buf);
                                write_buffer.extend_from_slice(buf_ref);
                                buf_ref = write_buffer.as_slice();

                                header_length = 8;
                            }

                            match self.stream.try_write(buf_ref){
                                Ok(n) => {
                                    // println!("[DEBUG] Write Event: ({}) {:?}", n, buf_ref);

                                    write_pos += n - header_length;
                                    if write_pos == buf.len() {
                                        write_flag = false;
                                        write_pos = 0;
                                        write_list.pop_front().unwrap();
                                    }
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    // 什么都不用做
                                }
                                Err(e) => {
                                    println!("[GQ] Fatal: {}, Self: {:?}", e, self);
                                    // 处理错误
                                    break;
                                }
                            }
                        }
                    }
                }

                if ready.is_readable() {
                    // 可以读
                    let mut buf = [0; 1024];
                    let mut buf_ref: &mut [u8] = &mut buf;
                    if read_flag && last_read_num < 1024 {
                        buf_ref = &mut buf[..last_read_num];
                    }
                    match self.stream.try_read(buf_ref) {
                        Ok(n) => {
                            let mut start_pos = 0;
                            if n == 0 {
                                continue;
                            }

                            // 没有在读取状态，进入读取状态
                            if !read_flag {
                                read_flag = true;
                                let mut small_buf = [0; 8];
                                small_buf.copy_from_slice(&buf[..8]);
                                last_read_num = u64::from_be_bytes(small_buf) as usize;
                                start_pos = 8;
                            }

                            read_buffer.extend_from_slice(&buf[start_pos..n]);
                            last_read_num -= n - start_pos;

                            // println!("[DEBUG] Read Event: ({}) {:?}", last_read_num, read_buffer);

                            // 读取按计划完成，提交数据并重新开新内存
                            if last_read_num == 0 {
                                read_flag = false;

                                callback_fn(read_buffer, self.clone()); // 把Buffer的所有权也送走
                                read_buffer = Vec::new();
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // 什么都不用做
                        }
                        Err(e) => {
                            println!("[GQ] Fatal: {}, Self: {:?}", e, self);
                            // 处理错误
                            break;
                        }
                    }
                }

                if ready.is_error() {
                    // 断了
                    println!("[GQ] Fatal (no msg), Self: {:?}", self);

                    // 处理错误
                    break;
                }
            }
        });
    }

    pub fn push_deque(self: Arc<Self>, data: Vec<u8>) {
        let mut data_list = self.send_deque.lock().unwrap();
        data_list.push_back(data);
    }
}


pub async fn start_tcp_server(port: u16, callback_fn: fn(Vec<u8>, Arc<TcpConnection>)) -> ! {
    let host_socket = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), port);
    let listener = tokio::net::TcpListener::bind(host_socket).await.unwrap();

    loop {
        let (client, client_sock_addr) = listener.accept().await.unwrap();

        let mut tcp_list = TCP_CONNECTIONS.write().unwrap();
        let conn = TcpConnection::new(client, client_sock_addr);
        let conn = Arc::new(conn);
        tcp_list.push(conn);

        let conn_index = tcp_list.len() - 1;
        drop(tcp_list);

        let tcp_list = TCP_CONNECTIONS.read().unwrap();
        let conn = tcp_list.get(conn_index).unwrap().clone();

        conn.start(callback_fn);
    }
}

pub async fn open_tcp_connection(addr: SocketAddr, callback_fn: fn(Vec<u8>, Arc<TcpConnection>)) -> anyhow::Result<Arc<TcpConnection>> {
    let listener = TcpStream::connect(addr).await?;

    let mut tcp_list = TCP_CONNECTIONS.write().unwrap();
    let conn = TcpConnection::new(listener, addr.try_into()?);
    let conn = Arc::new(conn);
    tcp_list.push(conn);

    let conn_index = tcp_list.len() - 1;
    drop(tcp_list);

    let tcp_list = TCP_CONNECTIONS.read().unwrap();
    let conn = tcp_list.get(conn_index).unwrap().clone();

    conn.clone().start(callback_fn);

    Ok(conn)
}
