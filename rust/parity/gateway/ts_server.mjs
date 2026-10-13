// TS oracle: the real startGatewayServer with the fake Gateway. Prints {url, wsUrl, token}.
// Usage: node ts_server.mjs <variant> <token|""> [staticAssetsPath]
import { startGatewayServer } from "../../../dist/src/gateway/server/GatewayServer.js";
import { createFakeGateway } from "./fake_gateway.mjs";
import { createFakePublicApprovals, createFakePublicHost } from "./fake_public.mjs";
const [variant = "full", token = "", staticAssetsPath] = process.argv.slice(2);
const gateway = createFakeGateway(variant);
const pub = variant === "full" ? { publicApprovals: createFakePublicApprovals(gateway.__log), publicHostCapabilities: createFakePublicHost(gateway.__log) } : {};
const server = await startGatewayServer({ gateway, ...pub, port: 0, ...(token ? { token } : {}), ...(staticAssetsPath ? { staticAssetsPath } : {}) });
console.log(JSON.stringify({ url: server.url, wsUrl: server.wsUrl, token: server.token, ...(server.tokenPath ? { tokenPath: server.tokenPath } : {}) }));
