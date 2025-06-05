#!/bin/bash

# Version information
EEG_UNINSTALL_VERSION="1.0.0"

# Exit on error with message
error_exit() {
    echo "❌ ERROR: $1"
    exit 1
}

# Function to print section headers
print_header() {
    echo ""
    echo "🔹 $1"
    echo "-------------------------------------------"
}

# Function to print success messages
print_success() {
    echo "✅ $1"
}

# Function to print warning messages
print_warning() {
    echo "⚠️ $1"
}

# Function to print info messages
print_info() {
    echo "ℹ️ $1"
}

# Function to safely remove a file if it exists
safe_remove() {
    if [ -f "$1" ]; then
        rm -f "$1" && print_success "Removed: $1" || print_warning "Failed to remove: $1"
    else
        print_info "File not found, skipping: $1"
    fi
}

# Function to safely remove a directory if it exists
safe_remove_dir() {
    if [ -d "$1" ]; then
        rm -rf "$1" && print_success "Removed directory: $1" || print_warning "Failed to remove directory: $1"
    else
        print_info "Directory not found, skipping: $1"
    fi
}

# Function to selectively remove lines from a file
remove_lines_containing() {
    local file="$1"
    local pattern="$2"
    
    if [ -f "$file" ]; then
        # Create a temporary file
        local temp_file=$(mktemp)
        
        # Filter out lines containing the pattern
        grep -v "$pattern" "$file" > "$temp_file" || true
        
        # Replace the original file
        mv "$temp_file" "$file"
        print_success "Removed lines containing '$pattern' from $file"
    else
        print_info "File not found, skipping: $file"
    fi
}

# Function to remove sections between marker comments
remove_marked_section() {
    local file="$1"
    local begin_marker="$2"
    local end_marker="$3"
    
    if [ -f "$file" ]; then
        # Check if markers exist in the file
        if grep -q "$begin_marker" "$file" && grep -q "$end_marker" "$file"; then
            # Create a temporary file
            local temp_file=$(mktemp)
            
            # Remove everything between and including the markers
            sed "/$begin_marker/,/$end_marker/d" "$file" > "$temp_file"
            
            # Replace the original file
            mv "$temp_file" "$file"
            print_success "Removed marked section from $file"
        else
            print_info "Markers not found in $file, skipping section removal"
        fi
    else
        print_info "File not found, skipping: $file"
    fi
}

# Function to comment out lines containing a pattern
comment_lines_containing() {
    local file="$1"
    local pattern="$2"
    
    if [ -f "$file" ]; then
        # Create a temporary file
        local temp_file=$(mktemp)
        
        # Comment out lines containing the pattern
        sed "s|^\(.*$pattern.*\)$|# \1 # Commented out by uninstall.sh|g" "$file" > "$temp_file"
        
        # Replace the original file
        mv "$temp_file" "$file"
        print_success "Commented out lines containing '$pattern' in $file"
    else
        print_info "File not found, skipping: $file"
    fi
}

# Function to restore a section in a file to default
restore_section() {
    local file="$1"
    local section_start="$2"
    local section_end="$3"
    local replacement="$4"
    
    if [ -f "$file" ]; then
        # Create a temporary file
        local temp_file=$(mktemp)
        
        # Extract the line numbers for the section
        local start_line=$(grep -n "$section_start" "$file" | cut -d: -f1)
        local end_line=$(grep -n "$section_end" "$file" | cut -d: -f1)
        
        if [ -n "$start_line" ] && [ -n "$end_line" ]; then
            # Extract the content before and after the section
            head -n $((start_line-1)) "$file" > "$temp_file"
            echo "$replacement" >> "$temp_file"
            tail -n +$((end_line+1)) "$file" >> "$temp_file"
            
            # Replace the original file
            mv "$temp_file" "$file"
            print_success "Restored section in $file"
        else
            print_warning "Section not found in $file"
            rm "$temp_file"
        fi
    else
        print_info "File not found, skipping: $file"
    fi
}

# Function to extract a field from a JSON object
extract_json_field() {
    local json="$1"
    local field="$2"
    
    echo "$json" | grep -o "\"$field\":\"[^\"]*\"" | sed "s/\"$field\":\"//" | tr -d '"' || echo ""
}

# Function to extract a JSON array from a JSON object
extract_json_array() {
    local json="$1"
    local field="$2"
    
    # Extract the array for the given field
    local array_content=$(echo "$json" | grep -o "\"$field\":\[[^]]*\]" | sed "s/\"$field\"://" || echo "[]")
    
    echo "$array_content"
}

# Function to handle a created file based on its uninstall actions
handle_created_file() {
    local file_json="$1"
    
    # Extract the path
    local path=$(extract_json_field "$file_json" "path")
    
    if [ -z "$path" ]; then
        print_warning "Invalid file entry in manifest (no path): $file_json"
        return
    fi
    
    print_info "Processing file: $path"
    
    # Extract uninstall actions
    local uninstall_actions=$(extract_json_array "$file_json" "uninstall_actions")
    
    # If no uninstall actions, just remove the file
    if [ "$uninstall_actions" = "[]" ] || [ -z "$uninstall_actions" ]; then
        safe_remove "$path"
        return
    fi
    
    # Check for remove action
    if [[ "$uninstall_actions" == *"\"remove\""* ]]; then
        safe_remove "$path"
        return
    fi
    
    # Check for remove_or_clean action
    if [[ "$uninstall_actions" == *"\"remove_or_clean\""* ]]; then
        # Extract patterns to remove
        local patterns_to_remove=$(extract_json_array "$file_json" "patterns_to_remove")
        
        if [ "$patterns_to_remove" = "[]" ] || [ -z "$patterns_to_remove" ]; then
            # No patterns, just remove the file
            safe_remove "$path"
        else
            # Clean up patterns
            patterns_to_remove=${patterns_to_remove#[}
            patterns_to_remove=${patterns_to_remove%]}
            
            # Split by commas and process each pattern
            IFS=',' read -ra PATTERNS <<< "$patterns_to_remove"
            for pattern in "${PATTERNS[@]}"; do
                # Remove quotes
                pattern=${pattern#\"}
                pattern=${pattern%\"}
                
                if [ -n "$pattern" ]; then
                    remove_lines_containing "$path" "$pattern"
                fi
            done
            
            # If file is now empty or only contains whitespace/comments, remove it
            if [ -f "$path" ]; then
                if [ ! -s "$path" ] || ! grep -v '^[[:space:]]*\(#.*\)\?$' "$path" > /dev/null; then
                    safe_remove "$path"
                    print_info "Removed empty file after pattern removal: $path"
                fi
            fi
        fi
        return
    fi
    
    # If we get here, no recognized action was found
    print_warning "Unknown uninstall action for $path: $uninstall_actions"
    safe_remove "$path"  # Default to removing the file
}

# Function to handle a modified file based on its patterns
handle_modified_file() {
    local file_json="$1"
    
    # Extract the path
    local path=$(extract_json_field "$file_json" "path")
    
    if [ -z "$path" ]; then
        print_warning "Invalid modified file entry in manifest (no path): $file_json"
        return
    fi
    
    print_info "Processing modified file: $path"
    
    # Check if file exists
    if [ ! -f "$path" ]; then
        print_info "Modified file not found, skipping: $path"
        return
    fi
    
    # Extract patterns to remove
    local patterns_to_remove=$(extract_json_array "$file_json" "patterns_to_remove")
    
    if [ "$patterns_to_remove" != "[]" ] && [ -n "$patterns_to_remove" ]; then
        # Clean up patterns
        patterns_to_remove=${patterns_to_remove#[}
        patterns_to_remove=${patterns_to_remove%]}
        
        # Split by commas and process each pattern
        IFS=',' read -ra PATTERNS <<< "$patterns_to_remove"
        for pattern in "${PATTERNS[@]}"; do
            # Remove quotes
            pattern=${pattern#\"}
            pattern=${pattern%\"}
            
            if [ -n "$pattern" ]; then
                remove_lines_containing "$path" "$pattern"
            fi
        done
    fi
    
    # Extract patterns to comment
    local patterns_to_comment=$(extract_json_array "$file_json" "patterns_to_comment")
    
    if [ "$patterns_to_comment" != "[]" ] && [ -n "$patterns_to_comment" ]; then
        # Clean up patterns
        patterns_to_comment=${patterns_to_comment#[}
        patterns_to_comment=${patterns_to_comment%]}
        
        # Split by commas and process each pattern
        IFS=',' read -ra PATTERNS <<< "$patterns_to_comment"
        for pattern in "${PATTERNS[@]}"; do
            # Remove quotes
            pattern=${pattern#\"}
            pattern=${pattern%\"}
            
            if [ -n "$pattern" ]; then
                comment_lines_containing "$path" "$pattern"
            fi
        done
    fi
}

# Function to parse the manifest file
parse_manifest() {
    local manifest_file="$1"
    
    if [ ! -f "$manifest_file" ]; then
        print_warning "Manifest file not found: $manifest_file"
        return 1
    fi
    
    # Read the manifest file
    local manifest_content=$(cat "$manifest_file")
    
    # Extract created files section
    local created_files_json=$(echo "$manifest_content" | grep -o '"created_files":\[[^]]*\]' | sed 's/"created_files"://')
    
    # Extract modified files section
    local modified_files_json=$(echo "$manifest_content" | grep -o '"modified_files":\[[^]]*\]' | sed 's/"modified_files"://')
    
    # If no created files section, set to empty array
    if [ -z "$created_files_json" ]; then
        created_files_json="[]"
    fi
    
    # If no modified files section, set to empty array
    if [ -z "$modified_files_json" ]; then
        modified_files_json="[]"
    fi
    
    # Save to global variables
    MANIFEST_CREATED_FILES="$created_files_json"
    MANIFEST_MODIFIED_FILES="$modified_files_json"
    
    return 0
}

# Function to process created files from manifest
process_created_files() {
    local created_files_json="$1"
    
    # If empty array, nothing to do
    if [ "$created_files_json" = "[]" ]; then
        print_info "No created files found in manifest."
        return
    fi
    
    # Remove brackets
    created_files_json=${created_files_json#[}
    created_files_json=${created_files_json%]}
    
    # Split into individual JSON objects
    # This is a bit tricky because we need to handle nested JSON
    # We'll use a simple approach that works for our specific format
    
    # Replace commas between objects with a special marker
    local processed_json=$(echo "$created_files_json" | sed 's/},{/@@@/g')
    
    # Split by the marker
    IFS='@@@' read -ra FILE_OBJECTS <<< "$processed_json"
    
    # Process each object
    for obj in "${FILE_OBJECTS[@]}"; do
        # Add back the curly braces if they were removed
        if [[ "$obj" != \{*\} ]]; then
            if [[ "$obj" == \{* ]]; then
                obj="${obj}}"
            elif [[ "$obj" == *\} ]]; then
                obj="{${obj}"
            else
                obj="{${obj}}"
            fi
        fi
        
        handle_created_file "$obj"
    done
}

# Function to process modified files from manifest
process_modified_files() {
    local modified_files_json="$1"
    
    # If empty array, nothing to do
    if [ "$modified_files_json" = "[]" ]; then
        print_info "No modified files found in manifest."
        return
    fi
    
    # Remove brackets
    modified_files_json=${modified_files_json#[}
    modified_files_json=${modified_files_json%]}
    
    # Split into individual JSON objects
    # This is a bit tricky because we need to handle nested JSON
    # We'll use a simple approach that works for our specific format
    
    # Replace commas between objects with a special marker
    local processed_json=$(echo "$modified_files_json" | sed 's/},{/@@@/g')
    
    # Split by the marker
    IFS='@@@' read -ra FILE_OBJECTS <<< "$processed_json"
    
    # Process each object
    for obj in "${FILE_OBJECTS[@]}"; do
        # Add back the curly braces if they were removed
        if [[ "$obj" != \{*\} ]]; then
            if [[ "$obj" == \{* ]]; then
                obj="${obj}}"
            elif [[ "$obj" == *\} ]]; then
                obj="{${obj}"
            else
                obj="{${obj}}"
            fi
        fi
        
        handle_modified_file "$obj"
    done
}

# Print banner
print_header "EEG System Uninstaller"
echo "🧹 EEG System Uninstaller v$EEG_UNINSTALL_VERSION"
echo "This script will uninstall the EEG system while preserving user modifications."
echo ""

# Confirm uninstallation
read -p "Are you sure you want to uninstall the EEG system? (y/n): " confirm
if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
    echo "Uninstallation cancelled."
    exit 0
fi

# Get current user
CURRENT_USER="$USER"
REPO_DIR="$HOME/elata-eeg"  # Repository directory
REPO_PATH=$(realpath "$REPO_DIR")
MANIFEST_FILE="$REPO_DIR/.install_manifest.json"  # Installation manifest file

print_header "Stopping Services and Processes"

# Stop and disable services
echo "Stopping and disabling systemd services..."
sudo systemctl stop daemon || print_warning "Failed to stop daemon service (it may not be running)"
sudo systemctl stop kiosk || print_warning "Failed to stop kiosk service (it may not be running)"
sudo systemctl disable daemon || print_warning "Failed to disable daemon service"
sudo systemctl disable kiosk || print_warning "Failed to disable kiosk service"

# Kill Chromium and Wayland compositor processes
echo "Killing Chromium and Wayland compositor processes..."
pkill -9 -f chromium-browser || print_info "No Chromium browser processes found"
pkill -9 -f chromium || print_info "No Chromium processes found"
pkill -9 -f labwc || print_info "No labwc compositor processes found"
sleep 2  # Give it time to terminate

# Function for fallback uninstallation
use_fallback_uninstall() {
    print_header "Using Fallback Uninstallation Method"
    
    print_header "Removing Systemd Services"
    # Remove systemd service files
    safe_remove "/etc/systemd/system/daemon.service"
    safe_remove "/etc/systemd/system/kiosk.service"
    
    # Reload systemd to apply changes
    sudo systemctl daemon-reload && print_success "Systemd configuration reloaded"
    
    print_header "Removing Binaries"
    # Remove installed binaries
    safe_remove "/usr/local/bin/eeg_daemon"
    
    print_header "Cleaning Configuration Files"
    # Clean up LXDE autostart configuration
    if [ -f "$HOME/.config/lxsession/LXDE-pi/autostart" ]; then
        # Check if the file was created by our installation
        if grep -q "chromium-browser --kiosk" "$HOME/.config/lxsession/LXDE-pi/autostart"; then
            # Remove the file if it appears to be our kiosk configuration
            safe_remove "$HOME/.config/lxsession/LXDE-pi/autostart"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.config/lxsession/LXDE-pi/autostart" "chromium-browser --kiosk"
        fi
    fi
    
    # Clean up labwc autostart configuration
    if [ -f "$HOME/.config/labwc/autostart" ]; then
        # Check if the file contains our marker section (old format)
        if grep -q "BEGIN ELATA-EEG SECTION" "$HOME/.config/labwc/autostart"; then
            # Remove only the section between our markers
            remove_marked_section "$HOME/.config/labwc/autostart" "BEGIN ELATA-EEG SECTION" "END ELATA-EEG SECTION"
            
            # If the file is now empty or only contains whitespace/comments, remove it
            if [ ! -s "$HOME/.config/labwc/autostart" ] || ! grep -v '^[[:space:]]*\(#.*\)\?$' "$HOME/.config/labwc/autostart" > /dev/null; then
                safe_remove "$HOME/.config/labwc/autostart"
                print_info "Removed empty autostart file after marker removal"
            fi
        # Check for new format (without markers)
        elif grep -q "chromium-browser --ozone-platform=wayland --kiosk" "$HOME/.config/labwc/autostart"; then
            # If it contains our kiosk configuration, remove the file
            safe_remove "$HOME/.config/labwc/autostart"
            print_info "Removed autostart file with kiosk configuration"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.config/labwc/autostart" "chromium-browser"
        fi
    fi
    
    # Clean up .xinitrc
    if [ -f "$HOME/.xinitrc" ]; then
        # Check if the file was created by our installation
        if grep -q "exec startlxde" "$HOME/.xinitrc" && [ $(wc -l < "$HOME/.xinitrc") -lt 5 ]; then
            # Remove the file if it appears to be our simple configuration
            safe_remove "$HOME/.xinitrc"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.xinitrc" "exec startlxde"
        fi
    fi
    
    # Clean up auto-login configuration
    safe_remove "/etc/systemd/system/getty@tty1.service.d/autologin.conf"
    
    # Clean up bash profile additions
    if [ -f "$HOME/.bash_profile" ]; then
        # Remove the lines we added for X11 autostart
        remove_lines_containing "$HOME/.bash_profile" "startx"
        remove_lines_containing "$HOME/.bash_profile" "bash_profile was sourced"
        remove_lines_containing "$HOME/.bash_profile" "Added by EEG System Installer"
    fi
    
    # Restore LightDM configuration if needed
    if [ -f "/etc/lightdm/lightdm.conf" ]; then
        # Comment out our specific settings rather than removing them
        comment_lines_containing "/etc/lightdm/lightdm.conf" "user-session=labwc"
        comment_lines_containing "/etc/lightdm/lightdm.conf" "autologin-session=labwc"
        comment_lines_containing "/etc/lightdm/lightdm.conf" "autologin-user=$CURRENT_USER"
        print_info "LightDM configuration has been modified. You may need to reconfigure it manually."
    fi
}

# Function for fallback uninstallation
use_fallback_uninstall() {
    print_header "Using Fallback Uninstallation Method"
    
    print_header "Removing Systemd Services"
    # Remove systemd service files
    safe_remove "/etc/systemd/system/daemon.service"
    safe_remove "/etc/systemd/system/kiosk.service"
    
    # Reload systemd to apply changes
    sudo systemctl daemon-reload && print_success "Systemd configuration reloaded"
    
    print_header "Removing Binaries"
    # Remove installed binaries
    safe_remove "/usr/local/bin/eeg_daemon"
    
    print_header "Cleaning Configuration Files"
    # Clean up LXDE autostart configuration
    if [ -f "$HOME/.config/lxsession/LXDE-pi/autostart" ]; then
        # Check if the file was created by our installation
        if grep -q "chromium-browser --kiosk" "$HOME/.config/lxsession/LXDE-pi/autostart"; then
            # Remove the file if it appears to be our kiosk configuration
            safe_remove "$HOME/.config/lxsession/LXDE-pi/autostart"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.config/lxsession/LXDE-pi/autostart" "chromium-browser --kiosk"
        fi
    fi
    
    # Clean up labwc autostart configuration
    if [ -f "$HOME/.config/labwc/autostart" ]; then
        # Check if the file contains our marker section
        if grep -q "BEGIN ELATA-EEG SECTION" "$HOME/.config/labwc/autostart"; then
            # Remove only the section between our markers
            remove_marked_section "$HOME/.config/labwc/autostart" "BEGIN ELATA-EEG SECTION" "END ELATA-EEG SECTION"
            
            # If the file is now empty or only contains whitespace/comments, remove it
            if [ ! -s "$HOME/.config/labwc/autostart" ] || ! grep -v '^[[:space:]]*\(#.*\)\?$' "$HOME/.config/labwc/autostart" > /dev/null; then
                safe_remove "$HOME/.config/labwc/autostart"
                print_info "Removed empty autostart file after marker removal"
            fi
        # Fallback for older installations without markers
        elif grep -q "chromium-browser --ozone-platform=wayland --kiosk" "$HOME/.config/labwc/autostart"; then
            # Remove the file if it appears to be our kiosk configuration
            safe_remove "$HOME/.config/labwc/autostart"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.config/labwc/autostart" "chromium-browser"
        fi
    fi
    
    # Clean up .xinitrc
    if [ -f "$HOME/.xinitrc" ]; then
        # Check if the file was created by our installation
        if grep -q "exec startlxde" "$HOME/.xinitrc" && [ $(wc -l < "$HOME/.xinitrc") -lt 5 ]; then
            # Remove the file if it appears to be our simple configuration
            safe_remove "$HOME/.xinitrc"
        else
            # Otherwise just remove our additions
            remove_lines_containing "$HOME/.xinitrc" "exec startlxde"
        fi
    fi
    
    # Clean up auto-login configuration
    safe_remove "/etc/systemd/system/getty@tty1.service.d/autologin.conf"
    
    # Clean up bash profile additions
    if [ -f "$HOME/.bash_profile" ]; then
        # Remove the lines we added for X11 autostart
        remove_lines_containing "$HOME/.bash_profile" "startx"
        remove_lines_containing "$HOME/.bash_profile" "bash_profile was sourced"
        remove_lines_containing "$HOME/.bash_profile" "Added by EEG System Installer"
    fi
    
    # Restore LightDM configuration if needed
    if [ -f "/etc/lightdm/lightdm.conf" ]; then
        # Comment out our specific settings rather than removing them
        comment_lines_containing "/etc/lightdm/lightdm.conf" "user-session=labwc"
        comment_lines_containing "/etc/lightdm/lightdm.conf" "autologin-session=labwc"
        comment_lines_containing "/etc/lightdm/lightdm.conf" "autologin-user=$CURRENT_USER"
        print_info "LightDM configuration has been modified. You may need to reconfigure it manually."
    fi
}

# Remove development mode flag
safe_remove "$HOME/.kiosk_dev_mode"

# Remove the manifest file itself
safe_remove "$MANIFEST_FILE"

print_header "Cleanup Complete"

print_success "The EEG system has been uninstalled."
print_info "Note: System packages (chromium-browser, npm, etc.) have been left intact."
print_info "If you want to remove these packages, you can do so manually using:"
print_info "  sudo apt remove chromium-browser npm curl git build-essential xserver-xorg x11-xserver-utils xinit lxde-core lxsession lxpanel pcmanfm"

echo ""
echo "Would you like to restart the display manager to apply changes? (y/n): "
read restart_dm
if [[ "$restart_dm" == "y" || "$restart_dm" == "Y" ]]; then
    sudo systemctl restart lightdm && print_success "Display manager restarted" || print_warning "Failed to restart display manager"
else
    print_info "Display manager not restarted. Changes will apply after reboot."
fi

echo ""
echo "🎉 Uninstallation complete!"