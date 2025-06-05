# Plan to Resolve `@alloc` TypeScript Build Error

**Date:** 2025-05-30

## 1. Problem Description

The Kiosk application build (`npm run build` in `kiosk/`) is failing with a TypeScript error:

*   Initially, it might appear as "Cannot find type definition file for '@alloc'".
*   More specifically, the error is "Type error: Cannot find type definition file for '@alloc'. The file is in the program because: Entry point for implicit type library '@alloc'".

This occurs despite the presence of a dummy declaration file at [`kiosk/src/types/alloc.d.ts`](../kiosk/src/types/alloc.d.ts). The command `npm ls @alloc` also fails, indicating `@alloc` is not a standard package.

## 2. Diagnosis

Through investigation, the following was determined:

*   A dependency, `@tailwindcss/postcss`, uses `@alloc/quick-lru`.
*   The package `@alloc/quick-lru` exists in `kiosk/node_modules/@alloc/quick-lru` and correctly provides its own `index.d.ts` type definition file.
*   The `typeRoots` configuration in both [`kiosk/tsconfig.json`](../kiosk/tsconfig.json) and [`applets/tsconfig.json`](../applets/tsconfig.json) includes `"./node_modules"` (or `"../kiosk/node_modules"`).
*   This broad inclusion in `typeRoots` likely causes TypeScript to scan the entire `node_modules` directory. When it encounters the scope directory `kiosk/node_modules/@alloc/`, it incorrectly identifies this *scope directory itself* as an "implicit type library" named `@alloc`, leading to the error.

## 3. Proposed Solution

The plan is to refine the `typeRoots` configuration to be more specific, preventing TypeScript from misinterpreting the `@alloc/` scope directory.

### Step 1: Modify `typeRoots` in `kiosk/tsconfig.json`

*   **File:** [`kiosk/tsconfig.json`](../kiosk/tsconfig.json)
*   **Current `compilerOptions.typeRoots`:** `["./node_modules/@types", "./node_modules"]`
*   **Proposed `compilerOptions.typeRoots`:** `