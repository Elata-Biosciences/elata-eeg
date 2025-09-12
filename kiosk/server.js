const { createServer } = require('http');
const { parse } = require('url');
const next = require('next');
const { createProxyMiddleware } = require('http-proxy-middleware');

const dev = process.env.NODE_ENV !== 'production';
const app = next({ dev, dir: __dirname });
const handle = app.getRequestHandler();

// Use explicit IP address instead of localhost to avoid DNS resolution issues
const target = 'http://127.0.0.1:9000';

const apiProxy = createProxyMiddleware({
  target,
  changeOrigin: true,
  secure: false, // Important for HTTP targets
  timeout: 30000, // 30 second timeout
  proxyTimeout: 30000, // 30 second proxy timeout
  logLevel: 'debug', // Add logging to see what's happening
  onError: (err, req, res) => {
    console.error('[API Proxy Error]:', err.message);
    console.error('[API Proxy Error] Target:', target);
    console.error('[API Proxy Error] URL:', req.url);
    if (!res.headersSent) {
      res.writeHead(502, {
        'Content-Type': 'application/json',
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type, Authorization'
      });
      res.end(JSON.stringify({
        error: 'Bad Gateway',
        message: 'Backend daemon connection failed',
        details: err.message,
        target: target
      }));
    }
  },
  onProxyReq: (proxyReq, req, res) => {
    console.log(`[API Proxy] ${req.method} ${req.url} -> ${target}${req.url}`);
  },
  onProxyRes: (proxyRes, req, res) => {
    console.log(`[API Proxy] Response: ${proxyRes.statusCode} for ${req.url}`);
  }
});

const sseProxy = createProxyMiddleware({
    target,
    changeOrigin: true,
    secure: false,
    timeout: 60000, // Longer timeout for SSE connections
    proxyTimeout: 60000,
    logLevel: 'debug',
    onProxyReq: (proxyReq, req, res) => {
        // Remove the 'Connection' header to allow for long-lived SSE connections
        proxyReq.removeHeader('Connection');
        console.log(`[SSE Proxy] ${req.method} ${req.url} -> ${target}${req.url}`);
    },
    onError: (err, req, res) => {
      console.error('[SSE Proxy Error]:', err.message);
      console.error('[SSE Proxy Error] Target:', target);
      console.error('[SSE Proxy Error] URL:', req.url);
      if (!res.headersSent) {
        res.writeHead(502, {
          'Content-Type': 'text/plain',
          'Access-Control-Allow-Origin': '*'
        });
        res.end('SSE Connection Failed: ' + err.message);
      }
    },
    onProxyRes: (proxyRes, req, res) => {
      console.log(`[SSE Proxy] Response: ${proxyRes.statusCode} for ${req.url}`);
    }
});

const wsProxy = createProxyMiddleware({
  target,
  ws: true,
  changeOrigin: true,
  secure: false,
  timeout: 30000,
  logLevel: 'debug',
  onError: (err, req, socket, head) => {
    console.error('[WS Proxy Error]:', err.message);
    console.error('[WS Proxy Error] Target:', target);
    socket.write('HTTP/1.1 502 Bad Gateway\r\n' +
                 'Content-Type: text/plain\r\n' +
                 '\r\n' +
                 'WebSocket Proxy Error: ' + err.message);
    socket.destroy();
  }
});

app.prepare().then(() => {
  createServer((req, res) => {
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
    const parsedUrl = parse(req.url, true);
    const { pathname } = parsedUrl;

    if (pathname.startsWith('/api/events')) {
        sseProxy(req, res);
    } else if (pathname.startsWith('/api')) {
      apiProxy(req, res);
    } else if (pathname.startsWith('/ws')) {
      wsProxy(req, res);
    } else {
      handle(req, res, parsedUrl);
    }
  }).listen(3000, (err) => {
    if (err) throw err;
    console.log('> Ready on http://localhost:3000');
  });
});