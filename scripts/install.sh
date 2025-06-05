#!/bin/bash

# Version information
EEG_INSTALL_VERSION="1.0.0"

# Enable SPI
echo "Navigate to "Interface Options" -> "SPI" -> Enable"
sudo raspi-config


# Make scripts executable
chmod +x scripts/rebuild.sh
chmod +x scripts/start.sh
chmod +x scripts/stop.sh
chmod +x scripts/uninstall.sh

# Configuration variables
REPO_DIR="$HOME/elata-eeg"  # Repository directory
CURRENT_USER="$USER"        # Current username
MANIFEST_FILE="$REPO_DIR/.install_manifest.json"  # Installation manifest file

set -e  # Exit on error

# Function to print section headers
print_header() {
    echo ""
    echo "🔹 $1"
    echo "-------------------------------------------"
}

# Function to track created files in the manifest
track_created_file() {
    local file="$1"
    local type="$2"
    local timestamp=$(date +"%Y-%m-%d %H:%M:%S")
    local uninstall_actions="$3"  # JSON array of uninstall actions
    local additional_props="$4"   # Additional JSON properties
    
    # Base JSON
    local json="{\"path\":\"$file\",\"type\":\"$type\",\"timestamp\":\"$timestamp\""
    
    # Add uninstall actions if provided
    if [ -n "$uninstall_actions" ]; then
        json="$json,\"uninstall_actions\":$uninstall_actions"
    fi
    
    # Add additional properties if provided
    if [ -n "$additional_props" ]; then
        json="$json,$additional_props"
    fi
    
    # Close JSON
    json="$json}"
    
    # Add to the manifest array
    CREATED_FILES+=("$json")
}

# Function to track modified files in the manifest
track_modified_file() {
    local file="$1"
    local modification="$2"
    local timestamp=$(date +"%Y-%m-%d %H:%M:%S")
    local patterns_type="$3"  # "remove" or "comment"
    local patterns="$4"       # JSON array of patterns
    
    # Base JSON
    local json="{\"path\":\"$file\",\"modification\":\"$modification\",\"timestamp\":\"$timestamp\""
    
    # Add patterns if provided
    if [ -n "$patterns_type" ] && [ -n "$patterns" ]; then
        if [ "$patterns_type" = "remove" ]; then
            json="$json,\"patterns_to_remove\":$patterns"
        elif [ "$patterns_type" = "comment" ]; then
            json="$json,\"patterns_to_comment\":$patterns"
        fi
    fi
    
    # Close JSON
    json="$json}"
    
    # Add to the manifest array
    MODIFIED_FILES+=("$json")
}

# Function to create a file with version comment
create_file_with_version() {
    local file="$1"
    local content="$2"
    local dir=$(dirname "$file")
    
    # Create directory if it doesn't exist
    mkdir -p "$dir"
    
    # Add version comment and write content
    echo "# Created by EEG System Installer v$EEG_INSTALL_VERSION" > "$file"
    echo "# $(date)" >> "$file"
    echo "$content" >> "$file"
    
    # Track the created file
    track_created_file "$file" "config"
}

# Initialize manifest arrays
CREATED_FILES=()
MODIFIED_FILES=()

# Get absolute path of repository directory (resolves ~)
REPO_PATH=$(realpath "$REPO_DIR")

print_header "Starting Installation"
echo "🚀 EEG System Installer v$EEG_INSTALL_VERSION"
echo "📂 Using repository path: $REPO_PATH"
echo "👤 Using username: $CURRENT_USER"

# Update and install dependencies
echo "📦 Installing necessary packages..."
sudo apt update
sudo apt remove chromium  # this fixes cpu rendering at 40% usage
sudo apt install mesa-utils vulkan-tools mesa-vulkan-drivers
sudo apt install -y chromium-browser npm curl git build-essential seatd libseat1 policykit-1

# Add user to required groups for Wayland/graphics access
echo "👥 Adding user to required groups for graphics access..."
sudo usermod -aG video,render $CURRENT_USER

# Enable seatd service for Wayland compositor
echo "🔄 Enabling seatd service for Wayland compositor..."
sudo systemctl enable --now seatd

# Install X11 and LXDE
echo "📦 Installing X11 and LXDE packages..."
sudo apt full-upgrade
sudo apt-get install -y xserver-xorg x11-xserver-utils xinit
sudo apt-get install -y lxde-core lxsession lxpanel pcmanfm

# Install Rust if not installed
if ! command -v cargo &> /dev/null; then
    echo "🦀 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Call rebuild.sh to build everything
echo "🔧 Building all components..."
./rebuild.sh from-install

# Create systemd service for the Rust daemon
echo "📝 Creating systemd service for Rust daemon..."
sudo tee /etc/systemd/system/daemon.service > /dev/null <<EOL
[Unit]
Description=Rust Daemon
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/eeg_daemon
Restart=always
User=$CURRENT_USER
Group=$CURRENT_USER
Environment=RUST_LOG=info
WorkingDirectory=$REPO_PATH/daemon
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=rust_daemon

[Install]
WantedBy=multi-user.target
EOL

# Track the created file
track_created_file "/etc/systemd/system/daemon.service" "service" "[\"remove\"]"

# Enable and start Rust daemon service
sudo systemctl daemon-reload
sudo systemctl enable daemon
sudo systemctl start daemon

# Note: Next.js kiosk setup is now handled by rebuild.sh

# Create systemd service for Next.js app
echo "📝 Creating systemd service for Next.js..."
sudo tee /etc/systemd/system/kiosk.service > /dev/null <<EOL
[Unit]
Description=Next.js Kiosk
After=network-online.target daemon.service
Wants=network-online.target
Requires=daemon.service

[Service]
ExecStart=/usr/bin/npm start --prefix $REPO_PATH/kiosk
Restart=always
User=$CURRENT_USER
Group=$CURRENT_USER
Environment=NODE_ENV=production
WorkingDirectory=$REPO_PATH/kiosk
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=nextjs_kiosk

[Install]
WantedBy=multi-user.target
EOL

# Track the created file
track_created_file "/etc/systemd/system/kiosk.service" "service" "[\"remove\"]"

# Enable and start Next.js service
sudo systemctl daemon-reload
sudo systemctl enable kiosk
sudo systemctl start kiosk

print_header "Configuring Kiosk Mode"
echo "🖥️ Configuring Chromium for kiosk mode..."

# Configure for X11/LXDE
mkdir -p ~/.config/lxsession/LXDE-pi

# Create LXDE autostart file with version comment
LXDE_AUTOSTART_CONTENT="@lxpanel --profile LXDE-pi
@pcmanfm --desktop --profile LXDE-pi

# Start Chromium in kiosk mode
@chromium-browser --kiosk --disable-infobars --disable-session-crashed-bubble --incognito http://localhost:3000"

create_file_with_version "$HOME/.config/lxsession/LXDE-pi/autostart" "$LXDE_AUTOSTART_CONTENT"
# Update the tracked file with uninstall actions
track_created_file "$HOME/.config/lxsession/LXDE-pi/autostart" "config" "[\"remove_or_clean\"]" "\"patterns_to_remove\":[\"chromium-browser --kiosk\"]"
chmod +x ~/.config/lxsession/LXDE-pi/autostart

# Create .xinitrc file for X11 with version comment
XINITRC_CONTENT="#!/bin/sh
exec startlxde"

create_file_with_version "$HOME/.xinitrc" "$XINITRC_CONTENT"
# Update the tracked file with uninstall actions
track_created_file "$HOME/.xinitrc" "config" "[\"remove_or_clean\"]" "\"patterns_to_remove\":[\"exec startlxde\"]"
chmod +x ~/.xinitrc

print_header "Configuring Auto-Login"
echo "🔑 Enabling auto-login for user '$CURRENT_USER'..."
sudo mkdir -p /etc/systemd/system/getty@tty1.service.d

# Create auto-login configuration with version comment
AUTOLOGIN_CONTENT="[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $CURRENT_USER --noclear %I 38400 linux"

# Add version comment and write to file
echo "# Created by EEG System Installer v$EEG_INSTALL_VERSION" | sudo tee /etc/systemd/system/getty@tty1.service.d/autologin.conf > /dev/null
echo "# $(date)" | sudo tee -a /etc/systemd/system/getty@tty1.service.d/autologin.conf > /dev/null
echo "$AUTOLOGIN_CONTENT" | sudo tee -a /etc/systemd/system/getty@tty1.service.d/autologin.conf > /dev/null

# Track the created file
track_created_file "/etc/systemd/system/getty@tty1.service.d/autologin.conf" "config" "[\"remove\"]"

# Track lightdm.conf modifications if it exists
if [ -f "/etc/lightdm/lightdm.conf" ]; then
    track_modified_file "/etc/lightdm/lightdm.conf" "Modified for auto-login" "comment" "[\"user-session=labwc\", \"autologin-session=labwc\", \"autologin-user=$CURRENT_USER\"]"
fi

print_header "Configuring X11 Autostart"
echo "🖥️ Configuring X11 to start automatically..."

# Check if .bash_profile exists
if [ -f "$HOME/.bash_profile" ]; then
    # No need to backup, just track the modification with patterns to remove
    track_modified_file "$HOME/.bash_profile" "Added X11 autostart configuration" "remove" "[\"startx\", \"bash_profile was sourced\", \"Added by EEG System Installer\"]"
fi

# Add X11 autostart and Wayland environment variables to .bash_profile
BASH_PROFILE_CONTENT="# Wayland environment variables
export WLR_BACKENDS=drm
export XDG_SESSION_TYPE=wayland

# Ensure XDG_RUNTIME_DIR is set properly
if [ -z \"\$XDG_RUNTIME_DIR\" ]; then
  export XDG_RUNTIME_DIR=/run/user/\$(id -u)
  mkdir -p \"\$XDG_RUNTIME_DIR\"
  chmod 0700 \"\$XDG_RUNTIME_DIR\"
fi

if [ -z \"\$DISPLAY\" ] && [ \"\$(tty)\" = \"/dev/tty1\" ]; then
    startx
fi
[ -f \"\$HOME/.bashrc\" ] && source \"\$HOME/.bashrc\"
echo \"bash_profile was sourced\""

# If file doesn't exist, create it with version comment
if [ ! -f "$HOME/.bash_profile" ]; then
    create_file_with_version "$HOME/.bash_profile" "$BASH_PROFILE_CONTENT"
else
    # Otherwise append to it
    echo "# Added by EEG System Installer v$EEG_INSTALL_VERSION" >> "$HOME/.bash_profile"
    echo "# $(date)" >> "$HOME/.bash_profile"
    echo "$BASH_PROFILE_CONTENT" >> "$HOME/.bash_profile"
    track_modified_file "$HOME/.bash_profile" "Added X11 autostart configuration" "remove" "[\"startx\", \"bash_profile was sourced\", \"Added by EEG System Installer\"]"
fi

# Create the installation manifest file
print_header "Creating Installation Manifest"
echo "📝 Creating installation manifest at $MANIFEST_FILE..."

# System packages array
SYSTEM_PACKAGES=(
  "chromium-browser"
  "npm"
  "curl"
  "git"
  "build-essential"
  "xserver-xorg"
  "x11-xserver-utils"
  "xinit"
  "lxde-core"
  "lxsession"
  "lxpanel"
  "pcmanfm"
)

# Runtime processes array
RUNTIME_PROCESSES=(
  "{\"name\":\"chromium-browser\",\"kill_command\":\"pkill -9 -f chromium-browser\",\"description\":\"Chromium browser kiosk process\"}"
  "{\"name\":\"chromium\",\"kill_command\":\"pkill -9 -f chromium\",\"description\":\"Chromium browser process\"}"
  "{\"name\":\"eeg_daemon\",\"kill_command\":\"sudo systemctl stop daemon\",\"description\":\"EEG daemon process\"}"
  "{\"name\":\"next.js\",\"kill_command\":\"sudo systemctl stop kiosk\",\"description\":\"Next.js kiosk process\"}"
)

# Post-uninstall actions
POST_UNINSTALL_ACTIONS=(
  "restart_display_manager"
  "reboot_recommended"
)

# Create JSON structure
MANIFEST_JSON="{
  \"version\": \"$EEG_INSTALL_VERSION\",
  \"timestamp\": \"$(date +"%Y-%m-%d %H:%M:%S")\",
  \"user\": \"$CURRENT_USER\",
  \"installation_path\": \"$REPO_PATH\",
  \"system_packages\": [$(printf "\"%s\"," "${SYSTEM_PACKAGES[@]}" | sed 's/,$//')],
  \"post_uninstall_actions\": [$(printf "\"%s\"," "${POST_UNINSTALL_ACTIONS[@]}" | sed 's/,$//')],
  \"runtime_processes\": [$(printf "%s," "${RUNTIME_PROCESSES[@]}" | sed 's/,$//')],"

# Add created files
if [ ${#CREATED_FILES[@]} -gt 0 ]; then
    MANIFEST_JSON="$MANIFEST_JSON
  \"created_files\": [$(printf "%s," "${CREATED_FILES[@]}" | sed 's/,$//')],"
else
    MANIFEST_JSON="$MANIFEST_JSON
  \"created_files\": [],"
fi

# Add modified files
if [ ${#MODIFIED_FILES[@]} -gt 0 ]; then
    MANIFEST_JSON="$MANIFEST_JSON
  \"modified_files\": [$(printf "%s," "${MODIFIED_FILES[@]}" | sed 's/,$//')]"
else
    MANIFEST_JSON="$MANIFEST_JSON
  \"modified_files\": []"
fi

MANIFEST_JSON="$MANIFEST_JSON
}"

# Write manifest to file
echo "$MANIFEST_JSON" > "$MANIFEST_FILE"
echo "✅ Installation manifest created"

print_header "Installation Complete"
echo "🎉 Setup complete! Rebooting..."
sudo sync # Ensure all disk writes are flushed before reboot
sudo reboot