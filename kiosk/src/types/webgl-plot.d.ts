// kiosk/src/types/webgl-plot.d.ts
/**
 * Manual type declarations for 'webgl-plot' (@next version) to work around
 * issues with package.json "exports" preventing automatic type resolution.
 * Declarations based on observed API in examples and .d.ts files.
 * REVERTING to multi-line API based on app/roll.js example.
 */
declare module 'webgl-plot' {

  // Based on ColorRGBA.d.ts
  export class ColorRGBA {
    r: number;
    g: number;
    b: number;
    a: number;
    constructor(r: number, g: number, b: number, a: number);
  }

  // Based on WbglLineRoll.d.ts and app/roll.js example (MULTI-LINE version)
  export class WebglLineRoll {
    // Constructor signature from example (wglp, width, numLines)
    constructor(wglp: WebglPlot, width: number, numLines: number);

    // Method from example (accepts array of arrays of numbers)
    addPoints(yArrays: number[][]): void;

    // Method from example (commented out due to runtime errors, but signature kept)
    setLineColor(color: ColorRGBA, lineIndex: number): void;

    // Method from example
    draw(): void;

    // NOTE: addPoint(yValue: number) does NOT exist in this multi-line version.
  }

  // Define WebglLine based on usage and example code
  export class WebglLine {
    constructor(color: ColorRGBA, numPoints: number);
    numPoints: number;
    color: ColorRGBA;
    lineWidth: number;
    scaleX: number;
    scaleY: number;
    offsetX: number;
    offsetY: number;
    xy: Float32Array;
    
    arrangeX(): void;
    setY(index: number, y: number): void;
    // Add any other methods or properties if discovered
  }

  // WebglStep might be an internal detail or a specific type of line.
  // For now, focusing on WebglLine as the primary type used.
  // If WebglStep is distinct and used, its definition can be refined.
  // export class WebglStep { ... } // Original WebglStep definition can be kept if needed separately

  // Based on webglplot.d.ts - Declaring used parts
  export class WebglPlot {
    gl: WebGL2RenderingContext;
    gScaleX: number;
    gScaleY: number;

    // Constructor signature used
    constructor(canvas: HTMLCanvasElement, options?: {
        antialias?: boolean;
        transparent?: boolean;
        powerPerformance?: "default" | "high-performance" | "low-power";
        deSync?: boolean;
        preserveDrawing?: boolean;
        debug?: boolean;
    });
    // Method used
    update(): void;
    // Method used
    clear(): void;
    // Added missing methods based on FftRenderer.tsx usage and example
    viewport(x: number, y: number, width: number, height: number): void;
    removeLine(line: WebglLine): void; // Changed to WebglLine
    addLine(line: WebglLine): void;    // Changed to WebglLine
    addAuxLine(line: WebglLine): void; // Added based on example
    removeAllLines(): void; // Added based on example (even if implemented via clear or loop)
  }
}