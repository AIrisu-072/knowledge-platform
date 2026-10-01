import { McpServer } from '@modelcontextprotocol/server';
import type { Client } from '@knowledge-platform/document-api-client';
import { registerDocumentTools } from './tools';
export { loadConfig } from './config';
export { createDocumentClient, verifyAgentSession, requestSignal, ORDINARY_DEADLINE_MS, COMPARISON_DEADLINE_MS } from './api';
export function createServer(client: Client): McpServer {
  const server = new McpServer({ name: 'document-mcp', version: '0.0.0' });
  registerDocumentTools(server, client);
  return server;
}
