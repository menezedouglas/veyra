import { invoke, isTauri } from '@tauri-apps/api/core'

export type Route = 'direct' | 'relay' | 'disconnected' | 'unknown'
export interface PeerSummary {
  id: string
  label: string
  fingerprint: string
  verified: boolean
  route: Route
  verificationCode?: string | null
}
export interface SessionStatus {
  phase: 'inactive' | 'active' | 'blocked'
  runtimeId?: string | null
  fingerprint?: string | null
  infrastructureConfigured: boolean
  reason?: string | null
  peers: PeerSummary[]
  pendingCount: number
  memoryProtection: 'not-checked' | 'protected' | 'unavailable'
  deliveries?: Delivery[]
}
export interface Invite { token: string; expiresInMs: number }
export interface PendingMessage { id: string; peerId: string; expiresInMs: number }
export interface OpenedMessage { body: string; displayForMs: number }
export interface Delivery { messageId?: string | null; peerId: string; state: 'sealed' | 'sent' | 'acknowledged' | 'delivery-unconfirmed' | 'rejected'; reason?: string | null }

export class DesktopRequiredError extends Error {
  constructor() { super('Abra o aplicativo Veyra para iniciar uma sessão. A visualização no navegador não tem acesso ao núcleo Rust.') }
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new DesktopRequiredError()
  return invoke<T>(name, args)
}

// A resposta de open_once é exclusiva desta chamada. Nunca encaminhar texto para
// eventos globais, stores, console, URLs ou o estado de diagnóstico.
export const api = {
  isDesktop: isTauri,
  status: () => command<SessionStatus>('connection_status'),
  startSession: (allowBackground = false, signalingEndpoint?: string) => command<SessionStatus>('start_session', { allowBackground, signalingEndpoint: signalingEndpoint || null }),
  endSession: () => command<void>('end_session'),
  createInvite: () => command<Invite>('create_invite'),
  joinInvite: (invite: string, label: string) => command<void>('join_invite', { invite, label }),
  verifyPeer: (peerId: string, code: string) => command<PeerSummary>('verify_peer', { peerId, code, approved: true }),
  sendMessage: (peerIds: string[], text: string) => command<Delivery[]>('send_message', { peerIds, text }),
  listPending: () => command<PendingMessage[]>('list_pending'),
  openOnce: (messageId: string) => command<OpenedMessage>('open_once', { messageId, confirmed: true }),
  discardMessage: (messageId: string) => command<void>('discard_message', { messageId }),
  revokePeer: (peerId: string) => command<void>('revoke_peer', { peerId }),
}
