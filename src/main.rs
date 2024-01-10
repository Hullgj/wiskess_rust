mod configs;
mod ops;
mod art;
mod scripts;

use crate::configs::config;
use crate::ops::{file_ops, exe_ops};
use crate::art::paths;
use crate::scripts::init;
use ops::valid_ops;
use serde_yaml::{self};
use std::fs::OpenOptions;
use std::env;
use clap::{Parser, ArgAction, Subcommand};
use chrono::{Utc, Duration};
use ctrlc;

/// Structure of the command line args
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// tool path, where binaries are stored. default gets from env var set internaly
    #[arg(short, long, default_value = "")]
    tool_path: String,
    /// Silent mode, no user input
    #[arg(short, long, action = ArgAction::SetTrue)]
    silent: bool,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// setup the wiskess dependencies
    Setup { },
    /// whipped pipeline process commands
    Whipped {
        /// config file of the artefact file paths and binaries to run as processors
        #[arg(short, long)]
        config: String,
        /// file path to the data source; either mounted or the root folder
        #[arg(short, long)]
        data_source_list: String,
        /// file path where the data is temporarily downloaded to and Wiskess output is stored locally
        #[arg(short, long)]
        local_storage: String,
        /// Start date - typically the earliest time of the incident, or a few days before
        #[arg(long)]
        start_date: String,
        /// End date - the current date or end of the incident timeframe
        #[arg(long)]
        end_date: String,
        /// IOC list file
        #[arg(short, long)]
        ioc_file: String,
        /// Either 'azure' or 'aws' - based on where the data source is stored.
        #[arg(short, long)]
        storage_type: String,
        /// The link that the data is stored on, i.e https://myaccount.file.core.windows.net/myclient/?sp=rl&st=...VWjgWTY8uc%3D&sr=s
        #[arg(long)]
        in_link: String,
        /// The link where you need the wiskess output uploaded to, 
        /// i.e. https://myaccount.file.core.windows.net/results/myclient/?sp=rcwl&st=2023-04-21T20...2FZWEA%3D&sr=s
        #[arg(long)]
        out_link: String,
        /// Set this flag to update the Wiskess results, such as changing the timeframe or after adding new IOCs to the list.
        #[arg(short, long)]
        update: bool,
        /// Set this flag to keep the downloaded data on your local storage. Useful if wanting to process the data after Wiskess. 
        /// Caution: make sure you have enough disk space for all the data source list.
        #[arg(short, long)]
        keep_evidence: bool,
    },
    /// process the data with wiskess
    Wiskess {
        /// config file of the artefact file paths and binaries to run as processors
        #[arg(short, long)]
        config: String,
        /// file path to the data source; either mounted or the root folder
        #[arg(short, long)]
        data_source: String,
        /// output folder that will be the destination of the processed results
        #[arg(short, long)]
        out_path: String,
        /// Start date - typically the earliest time of the incident, or a few days before
        #[arg(long)]
        start_date: String,
        /// End date - the current date or end of the incident timeframe
        #[arg(long)]
        end_date: String,
        /// IOC list file
        #[arg(short, long)]
        ioc_file: String,
    }
}

fn main() {
    // Set exit handler
    ctrlc::set_handler(move || {
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
 
    
    // Get the args
    let args = Args::parse();

    // Set tool path
    let mut tool_path = args.tool_path;
    if tool_path == "" {
        tool_path = format!("{}\\tools", env::current_dir().unwrap().display());
        println!("[ ] tool path: {}", tool_path);
    }

    match args.command {
        Commands::Setup { } => {
            // TODO: check if setup has been run, or if any binaries are missing
            init::run_setup(&tool_path);
        },
        Commands::Whipped { 
            config,
            data_source_list,
            local_storage,
            start_date,
            end_date,
            ioc_file,
            storage_type,
            in_link,
            out_link,
            update,
            keep_evidence,
        } => {            
            // Confirm date is valid
            let start_date = file_ops::check_date(start_date, &"start date".to_string());
            let end_date = file_ops::check_date(end_date, &"end date".to_string());

            // put the args into a whipped structure
            let args = config::WhippedArgs {
                config,
                data_source_list,
                local_storage,
                start_date,
                end_date,
                ioc_file,                
                storage_type,
                in_link,
                out_link,
                update,
                keep_evidence,
            };
            init::run_whipped(&tool_path, args)
        },
        Commands::Wiskess { 
            config, 
            data_source, 
            out_path, 
            start_date, 
            end_date, 
            ioc_file 
        } => {
            // Set output directories
            file_ops::make_folders(&out_path);
            
            // Set the start time
            let date_time_fmt = "%Y-%m-%dT%H%M%S";
            let wiskess_start = Utc::now();
            let wiskess_start_str = wiskess_start.format(date_time_fmt).to_string();
            
            // Set main log
            let out_log = format!("{}/wiskess_{}.log", &out_path, wiskess_start_str);
            file_ops::file_exists(&out_log, args.silent);
            
            // Write start time to log
            file_ops::log_msg(&out_log, format!("Starting wiskess at: {}", wiskess_start_str));

            
            // Confirm date is valid
            let start_date = file_ops::check_date(start_date, &"start date".to_string());
            let end_date = file_ops::check_date(end_date, &"end date".to_string());
            
            let main_args = config::MainArgs {
                out_path,
                start_date,
                end_date,
                tool_path,
                ioc_file,
                silent: args.silent
            };
        
            // Read the config
            let f: std::fs::File = OpenOptions::new()
                .read(true)
                .open(config)
                .expect("Unable to open config file.");
            let scrape_config: config::Config = serde_yaml::from_reader(f).expect("Could not read values.");
        
            // TODO: check or gracefully error when the yaml config misses keys
        
            // check the file paths in the config exist and return a hash of the art paths
            let data_paths = paths::check_art(
                scrape_config.artefacts, 
                &data_source,
                args.silent,
                &out_log
            );
            
            // Run in parallel then in series (if applicable) each binary of   
            // wiskers, enrichers and reporters
            for func in [
                &scrape_config.wiskers,
                &scrape_config.enrichers,
                &scrape_config.reporters] {
                    for num_threads in [0, 1] {
                        exe_ops::run_commands(func, &main_args, &data_paths, num_threads, &out_log);
                    }
            }

            // Validate wiskess has processed all input files into output files
            valid_ops::valid_process(&scrape_config.wiskers, &main_args, &data_paths, &out_log);

            // Set end time
            let wiskess_stop = Utc::now();
            let wiskess_duration = wiskess_stop - wiskess_start;
            let seconds = wiskess_duration.num_seconds() % 60;
            let minutes = (wiskess_duration.num_seconds() / 60) % 60;
            let hours = (wiskess_duration.num_seconds() / 60) / 60;
            let duration = format!("{:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds);
            file_ops::log_msg(
                &out_log, 
                format!(
                    "Wiskess finished at: {}, which took: {} [H:M:S]", 
                    wiskess_stop.format(date_time_fmt).to_string(), 
                    duration
                )
            );
        },
    }
}