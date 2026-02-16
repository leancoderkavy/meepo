import { useCallback, useEffect, useRef, useState } from 'react'

export interface GatewayEvent {
  event: string
  data: Record<string, unknown>
}

export interface GatewayResponse {
  id?: string
  result?: unknown
  error?: { code: number; message: string }
}

export type WsStatus = 'connecting' | 'connected' | 'disconnected'

export function useWebSocket(url: string) {
  const wsRef = useRef<WebSocket | null>(null)
  const [status, setStatus] = useState<WsStatus>('disconnected')
  const [events, setEvents] = useState<GatewayEvent[]>([])
  const pendingRef = useRef<Map<string, (resp: GatewayResponse) => void>>(new Map())
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>()
  const idCounter = useRef(0)

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return

    setStatus('connecting')
    const ws = new WebSocket(url)

    ws.onopen = () => {
      setStatus('connected')
    }

    ws.onclose = () => {
      setStatus('disconnected')
      wsRef.current = null
      reconnectTimer.current = setTimeout(connect, 3000)
    }

    ws.onerror = () => {
      ws.close()
    }

    ws.onmessage = (ev) => {
      try {
        const data = JSON.parse(ev.data)

        // Check if it's a broadcast event wrapping a response
        if (data.event === 'response' && data.data?.id) {
          const resolver = pendingRef.current.get(data.data.id)
          if (resolver) {
            resolver(data.data as GatewayResponse)
            pendingRef.current.delete(data.data.id)
            return
          }
        }

        // Regular event
        if (data.event) {
          setEvents((prev) => [...prev, data as GatewayEvent])
        }
      } catch {
        // ignore malformed messages
      }
    }

    wsRef.current = ws
  }, [url])

  useEffect(() => {
    connect()
    return () => {
      clearTimeout(reconnectTimer.current)
      wsRef.current?.close()
    }
  }, [connect])

  const send = useCallback(
    (method: string, params: Record<string, unknown> = {}): Promise<GatewayResponse> => {
      return new Promise((resolve, reject) => {
        if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) {
          reject(new Error('WebSocket not connected'))
          return
        }
        const id = `req_${++idCounter.current}`
        pendingRef.current.set(id, resolve)
        wsRef.current.send(JSON.stringify({ method, params, id }))

        // Timeout after 30s
        setTimeout(() => {
          if (pendingRef.current.has(id)) {
            pendingRef.current.delete(id)
            reject(new Error('Request timed out'))
          }
        }, 30000)
      })
    },
    [],
  )

  const clearEvents = useCallback(() => setEvents([]), [])

  return { status, events, send, clearEvents }
}
