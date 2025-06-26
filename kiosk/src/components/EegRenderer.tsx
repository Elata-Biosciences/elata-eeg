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
  const glRef     = useRef<WebGLRenderingContext | null>(null);
  const program   = useRef<WebGLProgram | null>(null);
  const location  = useRef<{
    pos: number; res: WebGLUniformLocation | null;
    sso: WebGLUniformLocation | null; col: WebGLUniformLocation | null;
  }>({pos:-1,res:null,sso:null,col:null});
  const vbos      = useRef<WebGLBuffer[]>([]);
  const cpuY      = useRef<Float32Array[]>([]);
  const rafId     = useRef<number>(0);

  const NCH   = config.channels.length;
  const NPTS  = config.samplesPerLine ?? 1024;
  const YSCL  = 1000000.0*(uiVoltageScaleFactor ?? 0.01);

  /* ---------- init (once) ---------- */
  useEffect(() => {
    if (!isActive || !canvasRef.current) return;

    const gl = canvasRef.current.getContext('webgl');
    if (!gl) return console.error('WebGL ctx failed');
    glRef.current = gl;

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
      vbos.current.forEach(b => gl.deleteBuffer(b));
      gl.deleteProgram(prog);
      vbos.current = []; cpuY.current = [];
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive]); // ← run once while active

  /* ---------- resize --------- */
  useEffect(() => {
    const gl = glRef.current; if (!gl) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(width  * dpr);
    const h = Math.round(height * dpr);
    const cvs = gl.canvas as HTMLCanvasElement;
    if (cvs.width !== w || cvs.height !== h) {
      cvs.width = w; cvs.height = h; gl.viewport(0,0,w,h);
    }
    if (location.current.res) gl.useProgram(program.current!),
      gl.uniform2f(location.current.res, w, h);
  }, [width, height]);

  /* ---------- render loop ---------- */
  useEffect(() => {
    if (!isActive || !glRef.current || !program.current) return;
    const gl = glRef.current;

    const draw = () => {
      // 1. ingest new EEG samples and shift data
      const chunks = dataBuffer.getAndClearData();
      if (chunks.length > 0) {
        const batches: number[][] = Array.from({ length: NCH }, () => []);
        chunks.forEach(chk =>
          chk.samples.forEach(s => batches[s.channelIndex].push(s.value))
        );

        for (let ch = 0; ch < NCH; ch++) {
          if (!batches[ch].length) continue;

          const ary = cpuY.current[ch]; // Interleaved (x,y,x,y,...) array
          const newVals = batches[ch];
          const numNew = newVals.length;

          if (numNew >= NPTS) {
            // If new data is more than the buffer can hold, just take the latest
            const latestVals = newVals.slice(-NPTS);
            for (let i = 0; i < NPTS; i++) {
              ary[i * 2 + 1] = latestVals[i]; // Update Y value
            }
          } else {
            const numExisting = NPTS - numNew;
            // Shift existing Y values to the left
            for (let i = 0; i < numExisting; i++) {
              ary[i * 2 + 1] = ary[(i + numNew) * 2 + 1];
            }
            // Append new Y values to the end
            for (let i = 0; i < numNew; i++) {
              ary[(numExisting + i) * 2 + 1] = newVals[i];
            }
          }

          // Upload the entire modified buffer
          gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
          gl.bufferSubData(gl.ARRAY_BUFFER, 0, ary);
        }
      }

      // 2. draw
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(program.current!);
      gl.enableVertexAttribArray(location.current.pos);
      gl.vertexAttribPointer(location.current.pos, 2, gl.FLOAT, false, 0, 0);

      const rowH = gl.canvas.height / NCH;
      for (let ch = 0; ch < NCH; ch++) {
        gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
        const yOff = rowH * (ch + 0.5);
        gl.uniform3f(location.current.sso!, gl.canvas.width / NPTS, YSCL, yOff);
        const [r,g,b] = getChannelColor(ch);
        gl.uniform4f(location.current.col!, r,g,b,1);
        gl.drawArrays(gl.LINE_STRIP, 0, NPTS);
      }

      rafId.current = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(rafId.current);
  }, [isActive, dataBuffer, NCH, NPTS, YSCL]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
});