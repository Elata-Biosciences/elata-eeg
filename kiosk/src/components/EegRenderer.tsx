'use client';

import React, {useRef, useEffect} from 'react';
import {getChannelColor} from '../utils/colorUtils';
import {useDataBuffer} from '../hooks/useDataBuffer';
import {SampleChunk} from '../types/eeg';

const VS = `
attribute vec2 a_xy;                // (sampleIndex, value)
uniform vec2  u_res;                // canvas resolution
uniform vec3  u_scrollScaleOffset;  // (xScale, yScale, yOffset)
void main() {
  vec2 pos = vec2(
    a_xy.x * u_scrollScaleOffset.x,             // scale time
    a_xy.y * u_scrollScaleOffset.y +            // scale EEG amp
      u_scrollScaleOffset.z);                   // move to channel band
  vec2 clip = (pos / u_res) * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}
`;

const FS = `
precision mediump float;
uniform vec4 u_color;
void main() { gl_FragColor = u_color; }
`;

interface Props {
  isActive: boolean;
  config: {channels: number[]; samplesPerLine?: number; ampScale?: number};
  dataBuffer: ReturnType<typeof useDataBuffer<SampleChunk>>;
  width: number;
  height: number;
  uiVoltageScaleFactor: number;
}

export const EegRenderer = React.memo(function EegRenderer({
  isActive,
  config,
  dataBuffer,
  width,
  height,
  uiVoltageScaleFactor,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  if (!isActive || !config?.channels?.length) {
    return <canvas ref={canvasRef} className="w-full h-full" />;
  }

  const glRef     = useRef<WebGLRenderingContext | null>(null);
  const program   = useRef<WebGLProgram | null>(null);
  const location  = useRef<{
    pos: number; res: WebGLUniformLocation | null;
    sso: WebGLUniformLocation | null; col: WebGLUniformLocation | null;
  }>({pos:-1,res:null,sso:null,col:null});
  const vbos      = useRef<WebGLBuffer[]>([]);
  const cpuY      = useRef<Float32Array[]>([]);
  const rafId     = useRef<number>(0);
  const dirtyChannels = useRef(new Set<number>());

  const NCH   = config.channels.length;
  const NPTS  = config.samplesPerLine ?? 1024;
  const YSCL  = 100000.0*(uiVoltageScaleFactor ?? 0.01);

  /* ---------- init (once) ---------- */
  useEffect(() => {
    if (!isActive || !canvasRef.current) return;

    const gl = canvasRef.current.getContext('webgl');
    if (!gl) return console.error('WebGL ctx failed');
    glRef.current = gl;
    // Prime u_res with current canvas size
    const cvs = gl.canvas as HTMLCanvasElement;
    gl.viewport(0, 0, cvs.width, cvs.height);

    // build program
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!; gl.shaderSource(s, src); gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? '');
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(prog) ?? '');
    program.current = prog;

    // locations
    location.current.pos = gl.getAttribLocation(prog, 'a_xy');
    location.current.res = gl.getUniformLocation(prog, 'u_res');
    location.current.sso = gl.getUniformLocation(prog, 'u_scrollScaleOffset');
    location.current.col = gl.getUniformLocation(prog, 'u_color');
  
    /* VBO per channel, interleaved (x,y) */
    for (let ch = 0; ch < NCH; ch++) {
      const buf = gl.createBuffer()!;
      const arr = new Float32Array(NPTS * 2);
      for (let i = 0; i < NPTS; i++) arr[i * 2] = i; // x
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, arr, gl.DYNAMIC_DRAW);
      vbos.current.push(buf);
      cpuY.current.push(arr); // keep same reference, we’ll mutate y’s
    }
  
    return () => {
      cancelAnimationFrame(rafId.current);
      const gl = glRef.current;
      const prog = program.current;
      if (gl && prog) {
        vbos.current.forEach(b => gl.deleteBuffer(b));
        gl.deleteProgram(prog);
      }
      vbos.current = [];
      cpuY.current = [];
      program.current = null;
      glRef.current = null;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive, NCH, NPTS]);

  /* ---------- resize --------- */
  useEffect(() => {
    const gl = glRef.current; if (!gl) return;
    gl.disable(gl.DEPTH_TEST);
    gl.clearColor(0,0,0,1);
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(width  * dpr);
    const h = Math.round(height * dpr);
    const cvs = gl.canvas as HTMLCanvasElement;
    if (cvs.width !== w || cvs.height !== h) {
      cvs.width = w; cvs.height = h; gl.viewport(0,0,w,h);
    }
    if (location.current.res) {
      gl.useProgram(program.current!);
      gl.uniform2f(location.current.res, w, h);
    }
  }, [width, height]);

  const processData = React.useCallback(() => {
    const chunks = dataBuffer.getAndClearData();
    if (chunks.length === 0) return;

    const newSamplesByChannel: number[][] = Array(NCH).fill(0).map(() => []);

    for (const chunk of chunks) {
      const { samples } = chunk;
      const numMetaChannels = NCH; // Assume channels are interleaved in order

      for (let i = 0; i < NCH; i++) {
        const channelIndex = i;
        for (let j = channelIndex; j < samples.length; j += numMetaChannels) {
          const value = samples[j];
          newSamplesByChannel[i].push(Math.abs(value) < 1e-10 ? 0 : value);
        }
      }
    }

    for (let i = 0; i < NCH; i++) {
      const newSamples = newSamplesByChannel[i];
      let numNew = newSamples.length;
      if (numNew === 0) continue;

      // Clamp to last NPTS samples to avoid negative offsets.
      if (numNew > NPTS) {
        newSamples.splice(0, numNew - NPTS);
        numNew = newSamples.length; // now <= NPTS
      }

      // In-place update to avoid GC churn.
      const ary = cpuY.current[i];
      // Shift left by numNew samples: move old tail to front.
      const shift = numNew * 2;
      if (shift < NPTS * 2) {
        ary.copyWithin(0, shift, NPTS * 2);
      }
      // Write new samples into the tail
      const base = (NPTS - numNew) * 2;
      for (let j = 0; j < numNew; j++) {
        const dst = base + j * 2;
        ary[dst + 1] = newSamples[j]; // y
      }
      // Fix x for all points (0..NPTS-1)
      for (let j = 0; j < NPTS; j++) {
        ary[j * 2] = j; // x
      }
      dirtyChannels.current.add(i);
    }
  }, [dataBuffer, NCH, NPTS, config.channels]);

  /* ---------- render loop ---------- */
  useEffect(() => {
    if (!isActive || !glRef.current || !program.current) return;
    const gl = glRef.current;

    const draw = () => {
      processData();

      for (const i of dirtyChannels.current) {
        const ary = cpuY.current[i];
        gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[i]);
        gl.bufferData(gl.ARRAY_BUFFER, ary, gl.DYNAMIC_DRAW);
      }
      dirtyChannels.current.clear();

      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.useProgram(program.current!);
      gl.enableVertexAttribArray(location.current.pos);

      const rowH = gl.canvas.height / NCH;
      for (let i = 0; i < NCH; i++) {
        gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[i]);
        // IMPORTANT: capture the currently bound buffer for the attribute
        gl.vertexAttribPointer(location.current.pos, 2, gl.FLOAT, false, 0, 0);
        const yOff = rowH * (i + 0.5);
        gl.uniform3f(location.current.sso!, gl.canvas.width / NPTS, YSCL, yOff);
        const [r,g,b] = getChannelColor(i);
        gl.uniform4f(location.current.col!, r,g,b,1);
        if (NPTS > 0) {
          gl.drawArrays(gl.LINE_STRIP, 0, NPTS);
        }
      }

      rafId.current = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(rafId.current);
  }, [isActive, dataBuffer, NCH, NPTS, YSCL, config.channels, processData]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
});