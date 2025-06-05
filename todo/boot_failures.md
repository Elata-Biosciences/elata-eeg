# Kiosk Boot Failure Investigation

## 1. Problem Description

After running `scripts/rebuild.sh` (which also runs `scripts/start.sh`) and then **manually rebooting** the system, the Chrome kiosk displays a "site can't be reached" error when trying to access `http://localhost:3000`.

## 2. Initial Investigation & Findings

### Scripts Reviewed:
*   [`ai_prompt.md`](./ai_prompt.md)
*   [`kiosk/ai_prompt.md`](./kiosk/ai_prompt.md)
*   [`scripts/rebuild.sh`](./scripts/rebuild.sh): Handles building the `eeg_daemon` and the Next.js kiosk application (including `npm run build` which creates the `kiosk/.next` directory). Calls `scripts/start.sh` if not run from `install.sh`.
*   [`scripts/start.sh`](./scripts/start.sh): Enables and starts `daemon.service` and `kiosk.service` using `systemctl`.
*   [`daemon/adc_daemon.service`](./daemon/adc_daemon.service): Service file for the backend daemon.
*   [`scripts/install.sh`](./scripts/install.sh): Installs dependencies, builds components (via `rebuild.sh`), and creates/enables `systemd` service files.

### Service Status (After Manual Reboot & Error):

*   **`daemon.service` (Backend):**
    *   Status: `active (running)`
    *   Logs: Indicate successful startup.

*   **`kiosk.service` (Frontend - Next.js):**
    *   Status: `failed (Result: exit-code)`
    *   Key Error Log: `[Error: Could not find a production build in the '.next' directory. Try building your app with 'next build' before starting the production server. https://nextjs.org/docs/messages/production-start-no-build-id]`

### `.next` Directory Status (After Manual Reboot & Error, Checked Manually):
*   `ls -ld /home/elata/elata-eeg/kiosk/.next`: `drwxr-xr-x 7 elata elata 4096 May 30 17:25 /home/elata/elata-eeg/kiosk/.next`
*   `ls -lA /home/elata/elata-eeg/kiosk/.next`: Shows a populated directory.

## 3. Root Cause Analysis & Revised Hypothesis

The `kiosk.service` log indicates it cannot find the production build in `kiosk/.next` *at the time of service startup during boot*. However, manual inspection *after* boot and failure shows the directory exists and is populated. This occurs after running `scripts/rebuild.sh` and then manually rebooting.

**Revised Hypothesis:**
The `npm run build` command within `scripts/rebuild.sh` completes, and `scripts/start.sh` can successfully start the kiosk. However, when the user manually reboots the system *after* `scripts/rebuild.sh` has finished, it's possible that not all file system writes for the newly created `kiosk/.next` directory were fully flushed from the disk cache to persistent storage. Upon reboot, `systemd` attempts to start `kiosk.service` very early. If the `kiosk/.next` directory is in a subtly inconsistent or incomplete state due to uncommitted writes, `npm start` fails to find a valid build ID.

## 4. Current Diagnostic Plan & Proposed Solution

```mermaid
graph TD
    A[Run `scripts/rebuild.sh`] --> B[Kiosk works initially via `scripts/start.sh`];
    B --> C[User Manually Reboots System];
    C --> D{Kiosk Unreachable Post-Reboot?};
    D -- Yes --> E[Check `kiosk.service` Status & Logs];
    E -- Logs show "No .next dir" --> F[Manually Verify `kiosk/.next` Directory Post-Reboot];
    F -- `ls -ld ...` & `ls -lA ...` --> G{Exists & Populated?};
    G -- Yes --> H[Hypothesis: Build artifacts from `rebuild.sh` not fully synced to disk before manual reboot, leading to transient inconsistency at boot];
    H -- Strongest Potential Fix --> I[Add `sync` after `npm run build` in `scripts/rebuild.sh`];
    I --> J[User re-runs `scripts/rebuild.sh`];
    J --> C;
```

**Detailed Steps & Solution:**

1.  **Verify `.next` Directory State After Manual Reboot (Completed):**
    *   Manual checks after a manual reboot (following a `rebuild.sh` run) and subsequent error show `kiosk/.next` exists and is populated. This points to a transient issue at boot time.

2.  **Implement Filesystem Sync in `rebuild.sh` (Proposed Solution):**
    *   To ensure all build artifacts from `npm run build` are written to disk before a potential manual reboot, add a `sync` command to `scripts/rebuild.sh` immediately after the `npm run build` step.
    *   **Action:** Modify `scripts/rebuild.sh`.
        ```diff
        --- a/scripts/rebuild.sh
        +++ b/scripts/rebuild.sh
        @@ -27,8 +27,10 @@
         # Rebuild kiosk
         echo "🧹 Cleaning Next.js build cache..."
         cd kiosk
        -rm -rf .next
+        rm -rf .next # Clean old build
         echo "⚙️ Rebuilding Next.js app..."
        -npm run build
        +npm run build # Create new build
        +echo "⚙️ Syncing kiosk build to disk..."
        +sync          # Ensure all build files are written to disk
         cd ..
         echo "✅ Kiosk rebuild complete!"
        ```
    *   *(Optional but good practice: Also add `sudo sync` before `sudo reboot` in `scripts/install.sh` for robustness during initial setup.)*

## 5. Next Steps

1.  **Confirm Plan:** User to confirm they are happy with this revised analysis and the proposed solution to modify `scripts/rebuild.sh`.
2.  **Implement Change:** If confirmed, switch to a mode capable of editing the script (e.g., "Code" mode) and apply the `sync` command to `scripts/rebuild.sh`.
3.  **Test:** User to run the modified `scripts/rebuild.sh`, then manually reboot the system.
4.  **Verify:** After the manual reboot, check if the kiosk starts correctly. If not, re-examine `kiosk.service` logs.