// kiosk/src/types/alloc.d.ts
/**
 * Dummy type declaration for '@alloc' to resolve "Cannot find type definition file" errors.
 * This may be needed if a transient dependency implicitly references '@alloc'
 * without providing its own type definitions or if it's an internal/optional module.
 */
declare module '@alloc' {
  // You can leave this empty if no specific types from '@alloc' are used,
  // or add specific type declarations if you know what they should be.
  // For now, an empty module should satisfy the TypeScript compiler.
}