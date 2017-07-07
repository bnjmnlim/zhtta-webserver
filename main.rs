// Benjamin Lim
// gash.rs
//
// Starting code for PA2
// Running on Rust 1.0.0 - build 02-21
//
// Brandeis University - cs146a - Spring 2015

extern crate getopts;

use getopts::{optopt, getopts};
use std::old_io::BufferedReader;
use std::process::{Command, Stdio};
use std::old_io::stdin;
use std::{old_io, os};
use std::str;
use std::thread;
use std::env;
use std::old_io::File;
use std::old_io::fs::PathExtensions;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::process::ChildStdout;
use std::process::ChildStdin;
use std::io::Write;
use std::io::Read;
use std::string::String;
use std::old_io::BufferedWriter;

pub struct Shell<'a> {
    cmd_prompt: &'a str,
}


struct message {
	info: [u8; 64],
	length: usize,
}

impl <'a>Shell<'a> {
    pub fn new(prompt_str: &'a str) -> Shell<'a> {
        Shell { cmd_prompt: prompt_str,}
    }

    fn run(&self) {
        let mut stdin = BufferedReader::new(stdin());
		//creates a string historystring, which records all user inputs
		let mut historystring = String::new();

        loop {
            old_io::stdio::print(self.cmd_prompt.as_slice());
            old_io::stdio::flush();

            let line = stdin.read_line().unwrap();
            let cmd_line = line.trim();
            let program = cmd_line.splitn(1, ' ').nth(0).expect("no program");
			//appends the user's input onto historystring and makes a new line
			historystring.push_str(cmd_line);
			historystring.push_str("\n");

            match program {
                ""      =>  { continue; }
                "exit"  =>  { return; }
                _       =>  { self.run_cmdline(cmd_line, &historystring); }
            }
        }
    }
	
	pub fn run_cmdline_out(&self, cmd_line: &str) -> String
	{
		let argv: Vec<&str> = cmd_line.split(' ').filter_map(|x| {
            if x == "" {
                None
            } else {
                Some(x)
            }
        }).collect();
		
		//checks and records if background mode should be used
		let mut bgm = 0;
		if argv[argv.len()-1] == "&"
		{
			bgm = 1;
		}
		
		//checks and records redirection, (0 is no redirection, 1 is >, and 2 is <)
		//saves the filename for redirection as redirtarget
		let mut redirtarget = String::new();
		let mut redirect = 0;
		let mut i = 0;
		let mut redirindex = 0;
		while i != argv.len()
		{
			if argv[i] == ">"
			{
				if redirect == 2
				{
					redirect = 3;
				}
				else
				{
					redirindex = i;
					redirect = 1;
				}
			}
			if argv[i] == "<"
			{
				if redirect == 1
				{
					redirect = 3;
				}
				else
				{
					redirindex = i;
					redirect = 2;
				}
			}
			if i == redirindex + 1
			{
				redirtarget = argv[i].to_string();
			}
			i = i + 1;
		}
		
		//separating the commands by pipes
		let cmds: Vec<&str> = cmd_line.split('|').collect();
		//for the last command, we have to check for redirection
		let finalstr = cmds[cmds.len()-1];
		let cmdfin: Vec<&str> = finalstr.split('>').collect();
		
		//creating the initial channels for the input for the first thread
		//also creating prevReciever and thisSender, which are mutable variables that hold
		//the proper channels for each command
		let (initSender, initReceiver) = channel::<message>();
		let mut prevReceiver = initReceiver;
		let firstSender = initSender.clone();
		let mut thisSender = initSender;
		
		i = 0;
		while i != cmds.len()
		{			
			let (currSender, currReceiver) = channel::<message>();
			thisSender = currSender;
			
			//case for the non-final command
			if i != cmds.len()-1
			{
				let command= cmds[i].to_string();
				thread::spawn(move || {
					let args: Vec<&str> = command.split(' ').filter_map(|x| {
						if x == "" {
							None
							} else {
							Some(x)
						}
					}).collect();
					
					match args[0]
					{
						"cd" 		=>	Shell::change_dir(args[1]),
						_			=> 	match args.first() {
											Some(&program) => Shell::run_cmd(program, args.tail(), prevReceiver, thisSender),
											None => (),
										},
					}
					
				});
			}
			else	//case for the final command, in which we have to check for redirection
			{
				//lastcmd is the last command minus any redirection
				let lastcmd = cmdfin[0].to_string();
				thread::spawn(move || {
					let args: Vec<&str> = lastcmd.split(' ').filter_map(|x| {
						if x == "" {
							None
						} else {
							if x == "&" {
								None
							} else {
								Some(x)
							}
						}
					}).collect();
					
					match args[0]
					{
						"cd" 		=>	Shell::change_dir(args[1]),
						_			=> 	match args.first() {
											Some(&program) => Shell::run_cmd(program, args.tail(), prevReceiver, thisSender),
											None => (),
										},
					}
				});
			}
			prevReceiver = currReceiver;
			i += 1;
		}
		
		//after all command threads are created, perform any necessary indirection operations
		if redirect == 2
		{
			//for indirect, we read in data using a buffered reader on the target file
			let mut currdir = env::current_dir().unwrap();
			currdir.push(redirtarget.as_slice());
			let file = File::open(&currdir);
			let mut reader = BufferedReader::new(file);
			let mut go = 0;
			while go == 0
			{
				let mut buf:[u8; 64] = [0; 64];
				let bufsize = match reader.read(&mut buf) {
					Ok(x)	=>	x,
					Err(x)	=>	break
				};
				let msg = message{info: buf, length:bufsize};
				firstSender.send(msg);
				if bufsize != 64
				{
					go = 1;
				}
			}
		}
		else	//if theres no indirection, we send an empty buffer to the first channel to start the entire process
		{
			let mut buf:[u8; 64] = [0; 64];
			let msg = message{info:buf, length:0};
			firstSender.send(msg);
		}
		//returning the string
		let mut result = String::new();
		let mut rununtil = 0;
		while rununtil == 0
		{
			let msg:message = prevReceiver.recv().unwrap();
			let curr_str = String::from_utf8_lossy(&msg.info);
			result = result + &curr_str;
			if msg.length != 64 {
				rununtil = 1;
			}
		}
		result
	}
	
	//added a string parameter, in case history is called
    fn run_cmdline(&self, cmd_line: &str, history: &String) {
		//splits the string according to spaces to find background mode (&) and redirection
        let argv: Vec<&str> = cmd_line.split(' ').filter_map(|x| {
            if x == "" {
                None
            } else {
                Some(x)
            }
        }).collect();
		
		//checks and records if background mode should be used
		let mut bgm = 0;
		if argv[argv.len()-1] == "&"
		{
			bgm = 1;
		}
		
		//checks and records redirection, (0 is no redirection, 1 is >, and 2 is <)
		//saves the filename for redirection as redirtarget
		let mut redirtarget = String::new();
		let mut redirect = 0;
		let mut i = 0;
		let mut redirindex = 0;
		while i != argv.len()
		{
			if argv[i] == ">"
			{
				if redirect == 2
				{
					redirect = 3;
				}
				else
				{
					redirindex = i;
					redirect = 1;
				}
			}
			if argv[i] == "<"
			{
				if redirect == 1
				{
					redirect = 3;
				}
				else
				{
					redirindex = i;
					redirect = 2;
				}
			}
			if i == redirindex + 1
			{
				redirtarget = argv[i].to_string();
			}
			i = i + 1;
		}
		
		//separating the commands by pipes
		let cmds: Vec<&str> = cmd_line.split('|').collect();
		//for the last command, we have to check for redirection
		let finalstr = cmds[cmds.len()-1];
		let cmdfin: Vec<&str> = finalstr.split('>').collect();
		
		//creating the initial channels for the input for the first thread
		//also creating prevReciever and thisSender, which are mutable variables that hold
		//the proper channels for each command
		let (initSender, initReceiver) = channel::<message>();
		let mut prevReceiver = initReceiver;
		let firstSender = initSender.clone();
		let mut thisSender = initSender;
		
		i = 0;
		while i != cmds.len()
		{			
			let (currSender, currReceiver) = channel::<message>();
			thisSender = currSender;
			
			//case for the non-final command
			if i != cmds.len()-1
			{
				let history_c = history.clone();
				let command= cmds[i].to_string();
				thread::spawn(move || {
					let args: Vec<&str> = command.split(' ').filter_map(|x| {
						if x == "" {
							None
							} else {
							Some(x)
						}
					}).collect();
					
					match args[0]
					{
						"cd" 		=>	Shell::change_dir(args[1]),
						"history"	=>	Shell::run_history(history_c, prevReceiver, thisSender),
						_			=> 	match args.first() {
											Some(&program) => Shell::run_cmd(program, args.tail(), prevReceiver, thisSender),
											None => (),
										},
					}
					
				});
			}
			else	//case for the final command, in which we have to check for redirection
			{
				//lastcmd is the last command minus any redirection
				let lastcmd = cmdfin[0].to_string();
				let history_c = history.clone();
				thread::spawn(move || {
					let args: Vec<&str> = lastcmd.split(' ').filter_map(|x| {
						if x == "" {
							None
						} else {
							if x == "&" {
								None
							} else {
								Some(x)
							}
						}
					}).collect();
					
					match args[0]
					{
						"cd" 		=>	Shell::change_dir(args[1]),
						"history"	=>	Shell::run_history(history_c, prevReceiver, thisSender),
						_			=> 	match args.first() {
											Some(&program) => Shell::run_cmd(program, args.tail(), prevReceiver, thisSender),
											None => (),
										},
					}
				});
			}
			prevReceiver = currReceiver;
			i += 1;
		}
		
		//after all command threads are created, perform any necessary indirection operations
		if redirect == 2
		{
			//for indirect, we read in data using a buffered reader on the target file
			let mut currdir = env::current_dir().unwrap();
			currdir.push(redirtarget.as_slice());
			let file = File::open(&currdir);
			let mut reader = BufferedReader::new(file);
			let mut go = 0;
			while go == 0
			{
				let mut buf:[u8; 64] = [0; 64];
				let bufsize = match reader.read(&mut buf) {
					Ok(x)	=>	x,
					Err(x)	=>	break
				};
				let msg = message{info: buf, length:bufsize};
				firstSender.send(msg);
				if bufsize != 64
				{
					go = 1;
				}
			}
		}
		else	//if theres no indirection, we send an empty buffer to the first channel to start the entire process
		{
			let mut buf:[u8; 64] = [0; 64];
			let msg = message{info:buf, length:0};
			firstSender.send(msg);
		}
		
		//for the case that background mode is not active
		if bgm == 0 
		{
			//if there is no redirection, we simply print from the final channel receiver
			if redirect != 1
			{
				let mut rununtil = 0;
				while rununtil == 0
				{
					let msg:message = prevReceiver.recv().unwrap();
					println!("{}", String::from_utf8_lossy(&msg.info));
					if msg.length != 64 {
						rununtil = 1;
					}
				}
			}
			if redirect == 1
			{
				//if there is redirection, we print to from the final channel reciever to a buffered writer
				let mut currdir = env::current_dir().unwrap();
				currdir.push(redirtarget.as_slice());
				let file = File::create(&currdir).unwrap();
				let mut writer = BufferedWriter::new(file);
				let mut go = 0;
				while go == 0
				{
					let msg:message = prevReceiver.recv().unwrap();
					writer.write(&msg.info);
					if msg.length != 64 {
						go = 1;
					}
				}
			}
		}
		else	//for the case in which we are prompted to use backgroundmode...
		{
			if redirect != 1
			{
				//we simply print to the console in a new thread instead of on the main thread
				thread::spawn(move || {
					let mut rununtil = 0;
					while rununtil == 0
					{
						let msg:message = prevReceiver.recv().unwrap();
						println!("{}", String::from_utf8_lossy(&msg.info));
						if msg.length != 64 {
							rununtil = 1;
						}
					}
				});
			} 
			else
			{
				//if redirected, we print on a buffered writer in a new thread
				thread::spawn(move || {
					let mut currdir = env::current_dir().unwrap();
					currdir.push(redirtarget.as_slice());
					let file = File::create(&currdir).unwrap();
					let mut writer = BufferedWriter::new(file);
					let mut go = 0;
					while go == 0
					{
						let msg:message = prevReceiver.recv().unwrap();
						writer.write(&msg.info);
						if msg.length != 64 {
							go = 1;
						}
					}
				});
			}
		}
    }
	
	//the function for cd
	fn change_dir(target: &str) {
		let mut currdir = env::current_dir().unwrap();
		if target == ".."
		{
			currdir.pop();
			env::set_current_dir(&currdir);
			println!("dir changed to: {}", env::current_dir().unwrap().display());
		}
		else
		{
			//we push any given viable path onto the current one.
			//Rust luckily automatically replaces the current path with absolute ones
			currdir.push(target);
			if currdir.exists()
			{
				env::set_current_dir(&currdir);
				println!("dir changed to: {}", env::current_dir().unwrap().display());
			}
			else
			{
				println!("dir stays at: {}", env::current_dir().unwrap().display());
			}
		}
	}
	
	//the history command 
	fn run_history(history: String, inputchannel: Receiver<message>, outputchannel: Sender<message>) {
		//we are passed the history string from the main loop, which we break down and send through a
		//buffer using channels to the next thread
		let bytes = history.as_bytes();
		let mut buf: [u8; 64] = [0; 64];
		let mut pos = 0;
		for b in bytes {
			buf[pos] = *b;
			pos += 1;
			if pos == 64
			{
				//fills up the buffer and sends it
				let msg = message{info: buf, length: 64};
				outputchannel.send(msg);
				pos = 0;
			}
		}
		//sends any remaining data left on the buffer
		let msg = message {info: buf, length: pos};
		outputchannel.send(msg);
	}

    fn run_cmd(program: &str, argv: &[&str], inputchannel: Receiver<message>, outputchannel: Sender<message>) {
        if Shell::cmd_exists(program) {
            let cmd = Command::new(program).args(argv)
				.stdin(Stdio::capture())
				.stdout(Stdio::capture())
				.stderr(Stdio::capture()).spawn().unwrap();
			//retrieving the child standard in and out
			let mut std_in = cmd.stdin.unwrap();
			let mut std_out = cmd.stdout.unwrap();
			
			//use a scoped thread to recieve inputs from the channel and put it into the child standard in
			//so that the main thread isn't blocked while it is waiting for more input
			thread::scoped(move || {
				let mut go = 0;
				while go == 0
				{
					let msg:message = match inputchannel.recv() {
						Err(x)	=>	break,
						Ok(x)	=>	x,
					};
					std_in.write(&msg.info[0..msg.length]);
					if msg.length != 64
					{
						go = 1;
					}
				}
			});
			
			//using a scoped thread to avoid blocking when writing
			thread::scoped(move || {
				let mut go = 0;
				while go == 0
				{
					let mut buf:[u8; 64] = [0; 64];
					let bufsize = std_out.read(&mut buf).unwrap();
					let msg = message{info: buf, length:bufsize};
					outputchannel.send(msg);
					if bufsize != 64
					{
						go = 1;
					}
				}
			});
			
        } else {
            println!("{}: command not found", program);
        }
    }

    fn cmd_exists(cmd_path: &str) -> bool {
        Command::new("which").arg(cmd_path).stdout(Stdio::capture()).status().unwrap().success()
    }
}

fn get_cmdline_from_args() -> Option<String> {
    /* Begin processing program arguments and initiate the parameters. */
    let args = os::args();

    let opts = &[
        getopts::optopt("c", "", "", "")
    ];

    getopts::getopts(args.tail(), opts).unwrap().opt_str("c")
}

fn main() {
    let opt_cmd_line = get_cmdline_from_args();

    match opt_cmd_line {
        Some(cmd_line) => Shell::new("").run_cmdline(cmd_line.as_slice(), &String::new()),
        None           => Shell::new("gash > ").run(),
    }
}
