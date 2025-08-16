//! PID file management for ensuring single daemon instance
//! 
//! This module provides functionality to create, check, and clean up PID files
//! to prevent multiple daemon instances from running simultaneously.

use std::fs::{File, OpenOptions};
use std::io::{Write, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process;
use std::os::unix::fs::OpenOptionsExt;

/// Manages PID file operations for single-instance enforcement
pub struct PidManager {
    pid_file_path: PathBuf,
}

impl PidManager {
    /// Create a new PID manager with the specified PID file path
    pub fn new<P: AsRef<Path>>(pid_file_path: P) -> Self {
        Self {
            pid_file_path: pid_file_path.as_ref().to_path_buf(),
        }
    }

    /// Acquire exclusive lock by creating PID file with current process ID
    /// Returns error if another instance is already running
    pub fn acquire_lock(&self) -> Result<(), String> {
        // Check if PID file already exists and if the process is still running
        if self.pid_file_path.exists() {
            match self.read_existing_pid() {
                Ok(existing_pid) => {
                    if self.is_process_running(existing_pid) {
                        return Err(format!(
                            "Another daemon instance is already running with PID {}. PID file: {}",
                            existing_pid,
                            self.pid_file_path.display()
                        ));
                    } else {
                        println!("Found stale PID file for non-running process {}. Cleaning up...", existing_pid);
                        if let Err(e) = self.cleanup_stale_pid() {
                            println!("Warning: Failed to cleanup stale PID file: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("Warning: Could not read existing PID file: {}. Attempting cleanup...", e);
                    if let Err(e) = self.cleanup_stale_pid() {
                        println!("Warning: Failed to cleanup invalid PID file: {}", e);
                    }
                }
            }
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = self.pid_file_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create PID file directory: {}", e))?;
            }
        }

        // Create PID file with current process ID
        let current_pid = process::id();
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644) // rw-r--r--
            .open(&self.pid_file_path)
            .map_err(|e| format!("Failed to create PID file: {}", e))?;

        write!(file, "{}", current_pid)
            .map_err(|e| format!("Failed to write PID to file: {}", e))?;

        file.sync_all()
            .map_err(|e| format!("Failed to sync PID file: {}", e))?;

        println!("PID file created: {} (PID: {})", self.pid_file_path.display(), current_pid);
        Ok(())
    }

    /// Release the lock by removing the PID file
    pub fn release_lock(&self) -> Result<(), String> {
        if self.pid_file_path.exists() {
            std::fs::remove_file(&self.pid_file_path)
                .map_err(|e| format!("Failed to remove PID file: {}", e))?;
            println!("PID file removed: {}", self.pid_file_path.display());
        }
        Ok(())
    }

    /// Check if another daemon instance is currently running
    pub fn is_running(&self) -> bool {
        if !self.pid_file_path.exists() {
            return false;
        }

        match self.read_existing_pid() {
            Ok(pid) => self.is_process_running(pid),
            Err(_) => false,
        }
    }

    /// Clean up stale PID file (when process is no longer running)
    pub fn cleanup_stale_pid(&self) -> Result<(), String> {
        if self.pid_file_path.exists() {
            std::fs::remove_file(&self.pid_file_path)
                .map_err(|e| format!("Failed to remove stale PID file: {}", e))?;
            println!("Stale PID file cleaned up: {}", self.pid_file_path.display());
        }
        Ok(())
    }

    /// Read the PID from an existing PID file
    fn read_existing_pid(&self) -> Result<u32, String> {
        let file = File::open(&self.pid_file_path)
            .map_err(|e| format!("Failed to open PID file: {}", e))?;

        let mut reader = BufReader::new(file);
        let mut pid_str = String::new();
        reader.read_line(&mut pid_str)
            .map_err(|e| format!("Failed to read PID file: {}", e))?;

        pid_str.trim().parse::<u32>()
            .map_err(|e| format!("Invalid PID in file: {}", e))
    }

    /// Check if a process with the given PID is currently running
    fn is_process_running(&self, pid: u32) -> bool {
        // On Unix systems, we can check if a process exists by sending signal 0
        // This doesn't actually send a signal but checks if the process exists
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None) {
            Ok(_) => true,  // Process exists
            Err(nix::errno::Errno::ESRCH) => false,  // No such process
            Err(_) => true,  // Other error (permission denied, etc.) - assume process exists
        }
    }

    /// Get the path to the PID file
    pub fn pid_file_path(&self) -> &Path {
        &self.pid_file_path
    }
}

impl Drop for PidManager {
    /// Automatically clean up PID file when PidManager is dropped
    fn drop(&mut self) {
        if let Err(e) = self.release_lock() {
            eprintln!("Warning: Failed to release PID lock during cleanup: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_pid_manager_creation() {
        let temp_dir = tempdir().unwrap();
        let pid_file = temp_dir.path().join("test.pid");
        let manager = PidManager::new(&pid_file);
        
        assert_eq!(manager.pid_file_path(), pid_file);
    }

    #[test]
    fn test_acquire_and_release_lock() {
        let temp_dir = tempdir().unwrap();
        let pid_file = temp_dir.path().join("test.pid");
        let manager = PidManager::new(&pid_file);
        
        // Should be able to acquire lock
        assert!(manager.acquire_lock().is_ok());
        assert!(pid_file.exists());
        
        // Should be able to release lock
        assert!(manager.release_lock().is_ok());
        assert!(!pid_file.exists());
    }

    #[test]
    fn test_double_lock_prevention() {
        let temp_dir = tempdir().unwrap();
        let pid_file = temp_dir.path().join("test.pid");
        
        let manager1 = PidManager::new(&pid_file);
        let manager2 = PidManager::new(&pid_file);
        
        // First manager should acquire lock successfully
        assert!(manager1.acquire_lock().is_ok());
        
        // Second manager should fail to acquire lock
        assert!(manager2.acquire_lock().is_err());
        
        // Clean up
        assert!(manager1.release_lock().is_ok());
    }

    #[test]
    fn test_stale_pid_cleanup() {
        let temp_dir = tempdir().unwrap();
        let pid_file = temp_dir.path().join("test.pid");
        
        // Create a PID file with a non-existent PID
        fs::write(&pid_file, "99999").unwrap();
        
        let manager = PidManager::new(&pid_file);
        
        // Should be able to acquire lock after cleaning up stale PID
        assert!(manager.acquire_lock().is_ok());
        assert!(pid_file.exists());
        
        // Clean up
        assert!(manager.release_lock().is_ok());
    }
}
