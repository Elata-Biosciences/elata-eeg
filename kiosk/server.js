const express = require('express');
const { createProxyMiddleware } = require('http-proxy-middleware');
const next = require('next');

const dev = process.env.NODE_ENV !== 'production';
const app = next({ dev });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  const server = express();

  // Proxy API requests to the backend
  server.use(
    '/api',
    createProxyMiddleware({
      target: 'http://127.0.0.1:9000',
      changeOrigin: true,
      pathRewrite: {
        '^/api': '/api', // keep the /api prefix
      },
    })
  );

  // Handle Next.js requests
  server.use((req, res) => {
    return handle(req, res);
  });

  const port = process.env.PORT || 3000;
  server.listen(port, (err) => {
    if (err) throw err;
    console.log(`> Ready on http://localhost:${port}`);
  });
});
