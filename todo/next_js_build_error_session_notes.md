# Session Notes - Next.js Build Error Investigation

## Problem Identified
- **Build Error**: Next.js build failing with "Cannot find module 'react' or its corresponding type declarations" in [`applets/brain_waves/ui/AppletFftRenderer.tsx:3`](../applets/brain_waves/ui/AppletFftRenderer.tsx:3)
- **Root Cause**: Applet files located outside the kiosk project directory cannot resolve React dependencies

## Architecture Understanding
- **Project Structure**: EEG system with multiple sub-projects (kiosk, daemon, driver, applets)
- **Dependency Chain**: 
  - [`kiosk/src/components/EegMonitor.tsx`](../kiosk/src/components/EegMonitor.tsx:20) imports [`BrainWavesDisplay`](../applets/brain_waves/ui/BrainWavesDisplay.tsx:1)
  - [`BrainWavesDisplay.tsx`](../applets/brain_waves/ui/BrainWavesDisplay.tsx:2) imports [`AppletFftRenderer`](../applets/brain_waves/ui/AppletFftRenderer.tsx:1)
  - [`AppletFftRenderer.tsx`](../applets/brain_waves/ui/AppletFftRenderer.tsx:3) tries to import React but can't find it

## Attempted Solutions
1. **Webpack Aliases**: Added React, React-DOM, and webgl-plot aliases in [`kiosk/next.config.ts`](../kiosk/next.config.ts:40)
2. **TypeScript Path Mapping**: Added module paths in [`kiosk/tsconfig.json`](../kiosk/tsconfig.json:22)
3. **Module Resolution**: Added module resolution paths to webpack config

## Current Status
- **Issue Persists**: TypeScript still cannot find React types for external applet files
- **Next Step**: Move applet components into kiosk project structure (was interrupted during this attempt)

## Files Modified
- [`kiosk/next.config.ts`](../kiosk/next.config.ts:1) - Added webpack aliases and module resolution
- [`kiosk/tsconfig.json`](../kiosk/tsconfig.json:1) - Added path mappings and typeRoots

## Recommended Next Steps
1. Complete moving applet components into `kiosk/src/components/applets/` directory
2. Update import paths in [`EegMonitor.tsx`](../kiosk/src/components/EegMonitor.tsx:20)
3. Test build after restructuring
4. Alternative: Create proper package structure with shared dependencies

## Key Insight
The fundamental issue is that Next.js builds from the `kiosk/` directory but tries to compile files in the `applets/` directory that reference dependencies not available in that location. The path mappings and webpack aliases help at runtime but TypeScript compilation still fails because it processes files relative to their actual location.