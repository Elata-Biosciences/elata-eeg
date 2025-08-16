const { createServer } = require('http');
const { parse } = require('url');
const next = require('next');
const { createProxyMiddleware } = require('http-proxy-middleware');

const dev = process.env.NODE_ENV !== 'production';
const app = next({ dev, dir: __dirname });
const handle = app.getRequestHandler();

const target = 'http://raspberrypi.local:9000';

const apiProxy = createProxyMiddleware({
  target,
  changeOrigin: true,
});

const sseProxy = createProxyMiddleware({
    target,
    changeOrigin: true,
    onProxyReq: (proxyReq, req, res) => {
        // Remove the 'Connection' header to allow for long-lived SSE connections
        proxyReq.removeHeader('Connection');
    },
});

const wsProxy = createProxyMiddleware({
  target,
  ws: true,
  changeOrigin: true,
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