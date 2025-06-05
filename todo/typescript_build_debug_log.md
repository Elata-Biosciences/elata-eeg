# TypeScript Build Debug Log & Strategy

This document tracks the debugging process for the persistent TypeScript build errors in the `eeg-kiosk` project, specifically the "Cannot find module 'react'" error when compiling applet files. It also outlines strategies to improve debugging efficiency and manage session context.

## 1. Current Problem

The Next.js build (`npm run build` in `kiosk/`) consistently fails with:
```
../applets/brain_waves/ui/AppletFftRenderer.tsx:3:71
Type error: Cannot find module 'react' or its corresponding type declarations.
```
This occurs despite `react` and `@types/react` being present in `kiosk/package.json` and presumably in `kiosk/node_modules/`.

## 2. History of Attempts (High-Level Summary)

We've tried several approaches to resolve TypeScript errors, including:

*   **Initial `typeRoots` Issues:**
    *   Encountered "Cannot find type definition file for '@alloc'" (and similar for `@img`, `@ampproject`, `@emnapi`, `@isaacs`, `@jridgewell`).
    *   Attempted to fix by adding `"./node_modules"` to `typeRoots` in `kiosk/tsconfig.json`.
    *   Attempted a workaround by creating dummy `index.d.ts` files in `kiosk/src/types/@scope_name/`. This was deemed unsustainable.
*   **Refining `tsconfig.json` Settings:**
    *   Removed `"./node_modules"` from `typeRoots`, leaving `["./node_modules/@types", "./src/types"]`. This led to a cascade of new "Cannot find type definition" errors in the IDE.
    *   Removed the `typeRoots` property entirely from both `kiosk/tsconfig.json` and `applets/tsconfig.json`, aiming to rely on default TypeScript resolution.
    *   Removed the `paths` override from `applets/tsconfig.json`.
    *   Removed the `baseUrl` override from `applets/tsconfig.json`.
    *   Modified `kiosk/tsconfig.json`'s `include` array to remove `../applets/**/*.ts` and `../applets/**/*.tsx`, aiming to give `applets/tsconfig.json` sole responsibility for applet files.

## 3. Current State of `tsconfig.json` Files (as of 2023-05-30)

*   **`kiosk/tsconfig.json`:**
    *   `compilerOptions`:
        *   `moduleResolution`: `"bundler"`
        *   `baseUrl`: `"."`
        *   `paths`: `{ "@/*": ["./src/*"], "webgl-plot": ["./node_modules/webgl-plot"] }`
        *   No `typeRoots` explicitly defined.
    *   `include`: Does *not* explicitly include `../applets/**` files anymore. Includes `next-env.d.ts`, `**/*.ts` (kiosk), `**/*.tsx` (kiosk), `.next/types/**/*.ts`.
    *   `exclude`: `["node_modules"]`
*   **`applets/tsconfig.json`:**
    *   `extends`: `"../kiosk/tsconfig.json"`
    *   `compilerOptions`: No overrides (inherits `baseUrl`, `paths`, `moduleResolution`, etc. from kiosk).
    *   `include`: `["**/*.ts", "**/*.tsx"]` (relative to `applets/`).

## 4. Current Hypothesis for "Cannot find module 'react'"

Despite the configurations, TypeScript's module resolution, when compiling files under `applets/` (via `applets/tsconfig.json` which extends `kiosk/tsconfig.json`), is not correctly resolving `react` from `kiosk/node_modules/`. The `baseUrl` inherited from `kiosk/tsconfig.json` should be `kiosk/`, making `node_modules/react` directly accessible. The persistence of this error suggests a subtle interaction or override we haven't identified, or perhaps an issue with how the Next.js build process itself handles these extended configurations for files outside its main `src` directory.

## 5. Strategies to Reduce Thrashing & Manage Context

To improve our debugging efficiency and manage context length:

*   **Structured Logging (This Document):** Maintain and refer to this log to track hypotheses, actions, and outcomes.
*   **Focused Experiments:** Make one isolated change at a time and test its direct impact.
*   **Context Summarization:** At the start of new interactions or if context grows, Roo should provide a concise summary of the current state, referencing this log.
*   **Targeted File Access:** Use `read_file` judiciously.
*   **User-Provided Checkpoints:** The user can help by summarizing or pointing to this log if Roo seems to be losing track or repeating steps.
*   **Modular Debugging:** If possible, isolate the problem. For example, create a minimal applet file that *only* imports React to see if the issue is general or specific to more complex files.
*   **Consider Environment State:** Be mindful of potentially stale `node_modules` or caches. A clean install (`rm -rf node_modules package-lock.json && npm install`) could be a last resort if corruption is suspected.
*   **IDE TypeScript Server Restarts:** Crucial after `tsconfig.json` changes.

## 6. Potential Next Steps for Debugging (When Resuming)

1.  **Verify `node_modules` Integrity:**
    *   Manually (or via `list_files`) check for the existence and basic structure of `kiosk/node_modules/react/` and `kiosk/node_modules/@types/react/`.
2.  **Minimal Test Case:**
    *   Create a new, very simple file like `applets/test-react-import/Test.tsx` with only `import React from 'react'; const Test = () => <div />; export default Test;`.
    *   Ensure `applets/tsconfig.json` would include this.
    *   See if this specific file reports the same "Cannot find module 'react'" error in the IDE or during build. This helps isolate if the issue is project-wide for applets or specific to `AppletFftRenderer.tsx`.
3.  **Investigate `applets/brain_waves/package.json`:**
    *   Currently, it's assumed applets don't need their own `react` dependency. Confirm if this `package.json` exists and if its presence might be interfering.
4.  **Next.js Build Internals (Research):**
    *   Briefly research if Next.js has specific known behaviors or requirements for compiling TypeScript files located outside its primary project root when using `tsconfig.json` `extends` and pathing.
5.  **Re-examine `paths` and `baseUrl` Interaction:**
    *   While we've simplified them, ensure the combination of `kiosk/tsconfig.json`'s `baseUrl: "."` and `applets/tsconfig.json` extending it correctly resolves modules from `kiosk/node_modules` for applet files. The expectation is that for an applet file, `import 'react'` should look into `kiosk/node_modules/react`.

---
*End of Log Entry for 2023-05-30*
---
## Log Entry: 2025-05-30

**Issue:** `Type error: Cannot find module 'react' or its corresponding type declarations.` in `../applets/brain_waves/ui/AppletFftRenderer.tsx`.

**Hypothesis from previous session (and confirmed by current investigation):**
The TypeScript compiler, guided by `kiosk/tsconfig.json` (via `applets/tsconfig.json`), cannot find the `react` module or its types when processing `applets/brain_waves/ui/AppletFftRenderer.tsx` because:
    a. The `baseUrl` is `kiosk`, and the file is outside this direct hierarchy without a specific `paths` mapping for `react`.
    b. Redundant `devDependencies` in the applet's `package.json` might confuse the resolution.

**Actions Taken:**

1.  **Modified `applets/brain_waves/package.json`**:
    *   Removed `react`, `react-dom`, `@types/react`, `@types/react-dom`, `typescript`, and `webgl-plot` from `devDependencies`. Kept only `@types/node`.
    *   **Reasoning:** The `kiosk` application is responsible for providing these as peer dependencies. This avoids potential conflicts or resolution issues from a nested `node_modules` in the applet.

2.  **Modified `kiosk/tsconfig.json`**:
    *   Added `paths` for `react` and `react-dom` to point to the type definitions within `kiosk/node_modules/@types/`.
    ```json
        "paths": {
          "@/*": ["./src/*"],
          "react": ["./node_modules/@types/react"],
          "react-dom": ["./node_modules/@types/react-dom"],
          "webgl-plot": ["./node_modules/webgl-plot"]
        },
    ```
    *   **Reasoning:** While Webpack in `next.config.ts` handles runtime aliases, TypeScript needs these `paths` to correctly resolve type declarations during compilation, especially for modules outside the `baseUrl` that are treated as peer dependencies.

**Next Steps:**
*   Attempt `npm run build` in the `kiosk` directory again.
*   If the error persists, restart the TypeScript server in the IDE.
*   If still an issue, consider a clean install (`rm -rf kiosk/node_modules && rm -rf applets/brain_waves/node_modules && cd kiosk && npm install`) to ensure no stale dependencies are interfering.
---
## Log Entry: 2025-05-30 (Continued)

**New Issue after previous fixes:**
Build now fails during "Collecting page data" with:
`TypeError: (0 , r.cache) is not a function`
Originating from `.next/server/chunks/169.js` and related to `/_not-found/page.js`.

**Hypothesis:**
The Webpack aliases for `react` and `react-dom` in `kiosk/next.config.ts`, which were intended to help applet module resolution, might be conflicting with Next.js's internal React handling, leading to this error. The `paths` in `kiosk/tsconfig.json` should suffice for TypeScript's type checking needs.

**Action Taken:**

1.  **Modified `kiosk/next.config.ts`**:
    *   Removed the Webpack aliases for `react` and `react-dom`. The alias for `webgl-plot` was retained.
    ```diff
    --- a/kiosk/next.config.ts
    +++ b/kiosk/next.config.ts
    @@ -46,11 +46,7 @@
         
         // Add aliases for dependencies used by applet files
         // @ts-ignore because alias can be an object or array
    -    config.resolve.alias['react'] = path.resolve(__dirname, 'node_modules/react');
    -    // @ts-ignore because alias can be an object or array
    -    config.resolve.alias['react-dom'] = path.resolve(__dirname, 'node_modules/react-dom');
    -    // @ts-ignore because alias can be an object or array
         config.resolve.alias['webgl-plot'] = path.resolve(__dirname, 'node_modules/webgl-plot');
         
         // Important: return the modified config
    ```
    *   **Reasoning:** To allow Next.js to handle its React dependency resolution naturally, which might resolve the `r.cache` error. Applets should still find React via peer dependency resolution.

**Next Steps:**
*   Attempt `npm run build` in the `kiosk` directory again.