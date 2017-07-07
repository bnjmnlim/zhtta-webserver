//
// zhtta.rs
//
// Starting code for PA3
// Revised to run on Rust 1.0.0 nightly - built 02-21
//
// Note that this code has serious security risks!  You should not run it 
// on any system with access to sensitive files.
// 
// Brandeis University - cs146a Spring 2015
// Dimokritos Stamatakis and Brionne Godby
// Version 1.0

// To see debug! outputs set the RUST_LOG environment variable, e.g.: export RUST_LOG="zhtta=debug"

#![feature(rustc_private)]
#![feature(libc)]
#![feature(io)]
#![feature(old_io)]
#![feature(old_path)]
#![feature(os)]
#![feature(core)]
#![feature(collections)]
#![feature(std_misc)]
#![allow(non_camel_case_types)]
#![allow(unused_must_use)]
#![allow(deprecated)]
#[macro_use]
extern crate log;
extern crate libc;

use std::io::*;
use std::old_io::File;
use std::{os, str};
use std::old_path::posix::Path;
use std::collections::hash_map::HashMap;
use std::borrow::ToOwned;
use std::thread::Thread;
use std::old_io::fs::PathExtensions;
use std::old_io::{Acceptor, Listener};
use std::collections::LinkedList;
use std::collections::BinaryHeap;
use std::cmp::Ord;
use std::cmp::Ordering;
use std::cmp::PartialOrd;
use std::cmp::PartialEq;
use std::fs::Metadata;
use std::collections::VecDeque;

extern crate getopts;
use getopts::{optopt, getopts};

use std::sync::{Arc, Mutex, Condvar};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc::channel;
use std::sync::Semaphore;

mod main;
use main::Shell;

static SERVER_NAME : &'static str = "Zhtta Version 1.0";

static IP : &'static str = "127.0.0.1";
static PORT : usize = 4414;
static WWW_DIR : &'static str = "./www";
static THREAD_CT : u32 = 20;
static CACHE_SIZE : usize = 1073741825;	//1GB of cache

static HTTP_OK : &'static str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n";
static HTTP_BAD : &'static str = "HTTP/1.1 404 Not Found\r\n\r\n";

static COUNTER_STYLE : &'static str = "<doctype !html><html><head><title>Hello, Rust!</title>
             <style>body { background-color: #884414; color: #FFEEAA}
                    h1 { font-size:2cm; text-align: center; color: black; text-shadow: 0 0 4mm red }
                    h2 { font-size:2cm; text-align: center; color: black; text-shadow: 0 0 4mm green }
             </style></head>
             <body>";

struct HTTP_Request {
    // Use peer_name as the key to access TcpStream in hashmap. 

    // (Due to a bug in extra::arc in Rust 0.9, it is very inconvenient to use TcpStream without the "Freeze" bound.
    //  See issue: https://github.com/mozilla/rust/issues/12139)
    peer_name: String,
    path: Path,
}

struct Cache_Entry {
	pathname: Path,
	size: usize,
	bufvec: Vec<[u8; 256]>,
}

impl Ord for HTTP_Request{
	fn cmp(&self, other:&Self) -> Ordering {
		let size1 = File::open(&self.path).unwrap().stat().unwrap().size;
		let size2 = File::open(&other.path).unwrap().stat().unwrap().size;
		if size1 > size2 {
			Ordering::Less
		} else if size1 < size2 {
			Ordering::Greater
		} else {
		Ordering::Equal
		}
	}
}

impl PartialOrd for HTTP_Request {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialEq for HTTP_Request {
	fn eq(&self, other: &Self) -> bool {
		let size1 = File::open(&self.path).unwrap().stat().unwrap().size;
		let size2 = File::open(&other.path).unwrap().stat().unwrap().size;
		if size1 == size2 {
			true
		} else {
			false
		}
	}
}

impl Eq for HTTP_Request {}

struct WebServer {
    ip: String,
    port: usize,
    www_dir_path: Path,
    
    request_queue_arc: Arc<Mutex<BinaryHeap<HTTP_Request>>>,
    stream_map_arc: Arc<Mutex<HashMap<String, std::old_io::net::tcp::TcpStream>>>,
	visit_count_arc: Arc<Mutex<u32>>,
	thread_count_arc: Arc<(Mutex<u32>, Condvar)>,
	cache_arc: Arc<Mutex<VecDeque<Cache_Entry>>>,
    
    notify_rx: Receiver<()>,
    notify_tx: Sender<()>,
}

impl WebServer {
    fn new(ip: String, port: usize, www_dir: String) -> WebServer {
        let (notify_tx, notify_rx) = channel();
        let www_dir_path = Path::new(www_dir);
        os::change_dir(&www_dir_path);

        WebServer {
            ip:ip,
            port: port,
            www_dir_path: www_dir_path,
                        
            request_queue_arc: Arc::new(Mutex::new(BinaryHeap::new())),
            stream_map_arc: Arc::new(Mutex::new(HashMap::new())),
			visit_count_arc: Arc::new(Mutex::new(0)),
			thread_count_arc: Arc::new((Mutex::new(0), Condvar::new())),
			cache_arc: Arc::new(Mutex::new(VecDeque::new())),
            
            notify_rx: notify_rx,
            notify_tx: notify_tx,
        }
    }
    
    fn run(&mut self) {
        self.listen();
        self.dequeue_static_file_request();
    }
    
    fn listen(&mut self) {
    	let addr = String::from_str(format!("{}:{}", self.ip, self.port).as_slice());
        let www_dir_path_str = self.www_dir_path.clone();
        let request_queue_arc = self.request_queue_arc.clone();
        let notify_tx = self.notify_tx.clone();
        let stream_map_arc = self.stream_map_arc.clone();
		let visit_count_arc = self.visit_count_arc.clone();

        Thread::spawn(move|| {
        	let listener = std::old_io::TcpListener::bind(addr.as_slice()).unwrap();
            let mut acceptor = listener.listen().unwrap();
            println!("{} listening on {} (serving from: {}).", 
                     SERVER_NAME, addr, www_dir_path_str.as_str().unwrap());
            for stream_raw in acceptor.incoming() {
                let (queue_tx, queue_rx) = channel();
                queue_tx.send(request_queue_arc.clone());
                
                let notify_chan = notify_tx.clone();
                let stream_map_arc = stream_map_arc.clone();
				let visitcount = visit_count_arc.clone();
                
                // Spawn a task to handle the connection.
                Thread::spawn(move|| {
					//the visitor count portion. Locks visitcount and unlocks it after obtaining visitor count number
					let curr_count;
					{
						let mut visitc = visitcount.lock().unwrap();
						*visitc += 1;
						curr_count = *visitc;
					}
					
                    let request_queue_arc = queue_rx.recv().unwrap();
                    let mut stream = match stream_raw {
                        Ok(s) => {s}
				        Err(e) => { panic!("Error getting the listener stream! {}", e) }
				    };
                    let peer_name = WebServer::get_peer_name(&mut stream);
                    debug!("Got connection from {}", peer_name);
                    let mut buf: [u8;500] = [0;500];
                    stream.read(&mut buf);
                    let request_str = match str::from_utf8(&buf){
                        Ok(s) => s,
                        Err(e)=> panic!("Error reading from the listener stream! {}", e),
                    };
                    debug!("Request:\n{}", request_str);
                    let req_group: Vec<&str> = request_str.splitn(3, ' ').collect();
                    if req_group.len() > 2 {
                        let path_str = ".".to_string() + req_group[1];
                        let mut path_obj = os::getcwd().unwrap();
                        path_obj.push(path_str.clone());
                        let ext_str = match path_obj.extension_str() {
                            Some(e) => e,
                            None => "",
                        };
                       
                        debug!("Requested path: [{}]", path_obj.as_str().expect("error"));
                        debug!("Requested path: [{}]", path_str);
                             
                        if path_str.as_slice().eq("./")  {
                            debug!("===== Counter Page request =====");
                            WebServer::respond_with_counter_page(stream, curr_count);
                            debug!("=====Terminated connection from [{}].=====", peer_name);
                        }  else if !path_obj.exists() || path_obj.is_dir() {
                            debug!("===== Error page request =====");
                            WebServer::respond_with_error_page(stream, &path_obj);
                            debug!("=====Terminated connection from [{}].=====", peer_name);
                        } else if ext_str == "shtml" { // Dynamic web pages.
                            debug!("===== Dynamic Page request =====");
                            WebServer::respond_with_dynamic_page(stream, &path_obj);
                            debug!("=====Terminated connection from [{}].=====", peer_name);
                        } else { 
                            debug!("===== Static Page request =====");
                            WebServer::enqueue_static_file_request(stream, &path_obj, stream_map_arc, request_queue_arc, notify_chan);
                        }
                    }
                });
            }
		});
    }

    fn respond_with_error_page(stream: std::old_io::net::tcp::TcpStream, path: &Path) {
		let mut stream = stream;
		let msg: String= format!("Cannot open: {}", path.as_str().expect("invalid path"));
		stream.write(HTTP_BAD.as_bytes());
		stream.write(msg.as_bytes());
    }

    //  Safe visitor counter.
    fn respond_with_counter_page(stream: std::old_io::net::tcp::TcpStream, count: u32) {
        let mut stream = stream;
        let response: String = 
            format!("{}{}<h1>Greetings, Krusty!</h1><h2>Visitor count: {}</h2></body></html>\r\n", 
                    HTTP_OK, COUNTER_STYLE, 
                    count );
        debug!("Responding to counter request");
        stream.write(response.as_bytes());
    }
    
    // Streaming file.
    //  Application-layer file caching.
    fn respond_with_static_file(stream: std::old_io::net::tcp::TcpStream, path: &Path, cache_arc_obj: Arc<Mutex<VecDeque<Cache_Entry>>>) {
        let mut stream = stream;
        let mut file_reader = File::open(path).unwrap();
		let mut in_cache = 0;

		let cache_arc_get = cache_arc_obj.clone();
		{
			let mut cache = cache_arc_get.lock().unwrap();
			let mut foundat = -1;
			let mut index = 0;
			for entry in cache.iter() {
				if entry.pathname == *path {
					//writing to stream
					stream.write(HTTP_OK.as_bytes());
					for buf in entry.bufvec.iter() {
						stream.write(buf);
					}
					in_cache = 1;
					foundat = index;
					break;
				}
				index += 1;
			}
			if in_cache == 1 {
				if foundat != 0 {
					println!("cache moving");
					let temp = cache.remove(foundat).unwrap();
					cache.push_front(temp);
				}
				println!("found in cache.");
			}
		}
		if in_cache == 0 {
			let mut tempvec = Vec::new();
			let mut tempsize = 0;
			stream.write(HTTP_OK.as_bytes());
			let mut file_end = 0;
			while file_end == 0 {
				let mut buf:[u8; 256] = [0; 256];
				let buflen = match file_reader.read(&mut buf){
                    Ok(x) => x,
                    Err(x) => break,
                };
				if buflen != 256 {
					file_end = 1;
				}
				tempvec.push(buf);
				tempsize = tempsize + 256;
				stream.write(&buf);
			}
			if tempsize < CACHE_SIZE {
				let mut cache = cache_arc_get.lock().unwrap();
				let mut cache_space = CACHE_SIZE;
				for entry in cache.iter() {
					cache_space -= entry.size;
				}
				while cache_space < tempsize {
					cache.pop_back();
					println!("file removed from cache");
					cache_space = CACHE_SIZE;
					for entry in cache.iter() {
						cache_space += entry.size;
					}
				}
				let temppath = path.clone();
				let newentry = Cache_Entry { pathname: temppath, size: tempsize, bufvec: tempvec };
				if newentry.size < CACHE_SIZE {
					cache.push_front(newentry);
				}
				println!("not in cache. input.");
			}
		}
    }
    
    // Server-side gashing.
    fn respond_with_dynamic_page(stream: std::old_io::net::tcp::TcpStream, path: &Path) {
		let mut stream = stream;
		let mut file_reader = File::open(path).unwrap();
		stream.write(HTTP_OK.as_bytes());
		let mut og_str = file_reader.read_to_string().unwrap();
		let vec: Vec<&str> = og_str.split_str("<!--").collect();
		let mut final_str = String::new();
		final_str.push_str(vec[0]);
		if vec.len() > 1
		{
			let curr_shell = Shell::new("");
			let mut x = 1;
			while x < vec.len()
			{
				let vec1: Vec<&str> = vec[x].split_str("-->").collect();
				let vec2: Vec<&str> = vec1[0].split_str("\"").collect();
				let cmd_str = curr_shell.run_cmdline_out(vec2[1]);
				final_str.push_str(cmd_str.as_slice());
				final_str.push_str(vec1[1]);
				x += 1;
			}
		}
		stream.write(final_str.as_bytes());
    }
    
    // Smarter Scheduling.
    fn enqueue_static_file_request(stream: std::old_io::net::tcp::TcpStream, path_obj: &Path, stream_map_arc: Arc<Mutex<HashMap<String, std::old_io::net::tcp::TcpStream>>>, req_queue_arc: Arc<Mutex<BinaryHeap<HTTP_Request>>>, notify_chan: Sender<()>) {
    	// Save stream in hashmap for later response.
        let mut stream = stream;
        let peer_name = WebServer::get_peer_name(&mut stream);
        let (stream_tx, stream_rx) = channel();
        stream_tx.send(stream);
        let stream = match stream_rx.recv(){
            Ok(s) => s,
            Err(e) => panic!("There was an error while receiving from the stream channel! {}", e),
        };
        let local_stream_map = stream_map_arc.clone();
        {   // make sure we request the lock inside a block with different scope, so that we give it back at the end of that block
            let mut local_stream_map = local_stream_map.lock().unwrap();
            local_stream_map.insert(peer_name.clone(), stream);
        }

        // Enqueue the HTTP request.
        // TOCHECK: it was ~path_obj.clone(), make sure in which order are ~ and clone() executed
        let req = HTTP_Request { peer_name: peer_name.clone(), path: path_obj.clone() };
        let (req_tx, req_rx) = channel();
        req_tx.send(req);
        debug!("Waiting for queue mutex lock.");
        
        let local_req_queue = req_queue_arc.clone();
        {   // make sure we request the lock inside a block with different scope, so that we give it back at the end of that block
            let mut local_req_queue = local_req_queue.lock().unwrap();
            let req: HTTP_Request = match req_rx.recv(){
                Ok(s) => s,
                Err(e) => panic!("There was an error while receiving from the request channel! {}", e),
            };
            local_req_queue.push(req);
            debug!("A new request enqueued, now the length of queue is {}.", local_req_queue.len());
            notify_chan.send(()); // Send incoming notification to responder task. 
        }
    }
    
    // Smarter Scheduling.
    fn dequeue_static_file_request(&mut self) {
        let req_queue_get = self.request_queue_arc.clone();
        let stream_map_get = self.stream_map_arc.clone();
		
        // Receiver<> cannot be sent to another task. So we have to make this task as the main task that can access self.notify_rx.
        let (request_tx, request_rx) = channel();
        loop {
            self.notify_rx.recv();    // waiting for new request enqueued.
            {   // make sure we request the lock inside a block with different scope, so that we give it back at the end of that block
                let mut req_queue = req_queue_get.lock().unwrap();
                if req_queue.len() > 0 {
                    let req = req_queue.pop().unwrap();
                    debug!("A new request dequeued, now the length of queue is {}.", req_queue.len());
                    request_tx.send(req);
                }
            }

            let request = match request_rx.recv(){
                Ok(s) => s,
                Err(e) => panic!("There was an error while receiving from the request channel! {}", e),
            };
            // Get stream from hashmap.
            let (stream_tx, stream_rx) = channel();
            {   // make sure we request the lock inside a block with different scope, so that we give it back at the end of that block
                let mut stream_map = stream_map_get.lock().unwrap();
                let stream = stream_map.remove(&request.peer_name).expect("no option tcpstream");
                stream_tx.send(stream);
            }
            // TODO: Spawning more tasks to respond the dequeued requests concurrently. You may need a semophore to control the concurrency.
            let stream = match stream_rx.recv(){
                Ok(s) => s,
                Err(e) => panic!("There was an error while receiving from the stream channel! {}", e),
            };
			let pname = request.peer_name.clone();
			
			//using a Condvar to notify wait threads that attempt to start when there are more than THREAD_CT active threads
			let thread_count = self.thread_count_arc.clone();
			let cache_arc_get = self.cache_arc.clone();
			Thread::spawn(move || {
				let &(ref num, ref cvar) = &*thread_count;
				{
					let start = num.lock().unwrap();
					let mut start = if *start >= THREAD_CT {
							cvar.wait(start).unwrap()
						} else {
							start
						};
					*start += 1;
					println!("num threads: {}", *start);
				}
				WebServer::respond_with_static_file(stream, &request.path, cache_arc_get);
				{
					let mut decrement = num.lock().unwrap();
					*decrement -= 1;
				}
				cvar.notify_one();
			});
           
            // Close stream automatically.
            debug!("=====Terminated connection from [{}].=====", pname);
        }
    }
    
    fn get_peer_name(stream: &mut std::old_io::net::tcp::TcpStream) -> String{
        match stream.peer_name() {
            Ok(s) => {format!("{}:{}", s.ip, s.port)}
            Err(e) => {panic!("Error while getting the stream name! {}", e)}
        }
    }
}

fn get_args() -> (String, usize, String) {
	fn print_usage(program: &str) {
        println!("Usage: {} [options]", program);
        println!("--ip     \tIP address, \"{}\" by default.", IP);
        println!("--port   \tport number, \"{}\" by default.", PORT);
        println!("--www    \tworking directory, \"{}\" by default", WWW_DIR);
        println!("-h --help \tUsage");
    }
    
    /* Begin processing program arguments and initiate the parameters. */
    let args = os::args();
    let program = args[0].clone();
    
    let opts = [
        getopts::optopt("", "ip", "The IP address to bind to", "IP"),
        getopts::optopt("", "port", "The Port to bind to", "PORT"),
        getopts::optopt("", "www", "The www directory", "WWW_DIR"),
        getopts::optflag("h", "help", "Display help"),
    ];

    let matches = match getopts::getopts(args.tail(), &opts) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_err_msg()) }
    };

    if matches.opt_present("h") || matches.opt_present("help") {
        print_usage(program.as_slice());
        unsafe { libc::exit(1); }
    }
    
    let ip_str = if matches.opt_present("ip") {
                    matches.opt_str("ip").expect("invalid ip address?").to_owned()
                 } else {
                    IP.to_owned()
                 };
    
    let port:usize = if matches.opt_present("port") {
        let input_port = matches.opt_str("port").expect("Invalid port number?").trim().parse::<usize>().ok();
        match input_port {
            Some(port) => port,
            None => panic!("Invalid port number?"),
        }
    } else {
        PORT
    };
    
    let www_dir_str = if matches.opt_present("www") {
                        matches.opt_str("www").expect("invalid www argument?") 
                      } else { WWW_DIR.to_owned() };
    
    (ip_str, port, www_dir_str)
}

fn main() {
    let (ip_str, port, www_dir_str) = get_args();
    let mut zhtta = WebServer::new(ip_str, port, www_dir_str);
    zhtta.run();
}
