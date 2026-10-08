// Boots the host simulation wasm inside a dedicated module Web Worker.
import init, { host_worker_start } from './pkg/host.js';

await init();
host_worker_start();
