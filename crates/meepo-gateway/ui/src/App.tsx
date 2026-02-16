import { useCallback, useEffect, useRef, useState } from 'react'
import ChatInput from './components/ChatInput'
import ChatMessage from './components/ChatMessage'
import SessionSidebar from './components/SessionSidebar'
import TypingIndicator from './components/TypingIndicator'
import { useWebSocket } from './hooks/useWebSocket'

interface Message {
  role: 'user' | 'assistant'
  content: string
}

interface Session {
  id: string
  name: string
  message_count: number
}

const WS_URL = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`

export default function App() {
  const { status, events, send } = useWebSocket(WS_URL)
  const [messages, setMessages] = useState<Message[]>([])
  const [sessions, setSessions] = useState<Session[]>([{ id: 'main', name: 'Main', message_count: 0 }])
  const [activeSession, setActiveSession] = useState('main')
  const [isTyping, setIsTyping] = useState(false)
  const [activeTool, setActiveTool] = useState<string | undefined>()
  const messagesEndRef = useRef<HTMLDivElement>(null)

  // Auto-scroll to bottom
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, isTyping])

  // Process gateway events
  useEffect(() => {
    for (const evt of events) {
      switch (evt.event) {
        case 'message.received': {
          const data = evt.data as { content: string; session_id: string; role?: string }
          if (data.session_id === activeSession) {
            setMessages((prev) => [...prev, { role: (data.role as 'assistant') || 'assistant', content: data.content }])
          }
          setIsTyping(false)
          setActiveTool(undefined)
          break
        }
        case 'typing.start':
          if ((evt.data as { session_id: string }).session_id === activeSession) {
            setIsTyping(true)
          }
          break
        case 'typing.stop':
          if ((evt.data as { session_id: string }).session_id === activeSession) {
            setIsTyping(false)
          }
          break
        case 'tool.executing':
          if ((evt.data as { session_id: string }).session_id === activeSession) {
            setActiveTool((evt.data as { tool: string }).tool)
          }
          break
        case 'session.created': {
          const s = evt.data as Session
          setSessions((prev) => [...prev, s])
          break
        }
      }
    }
  }, [events, activeSession])

  // Load sessions on connect
  useEffect(() => {
    if (status === 'connected') {
      send('session.list').then((resp) => {
        if (resp.result && Array.isArray(resp.result)) {
          setSessions(resp.result as Session[])
        }
      }).catch(() => {})
    }
  }, [status, send])

  const handleSend = useCallback(
    async (content: string) => {
      setMessages((prev) => [...prev, { role: 'user', content }])
      setIsTyping(true)
      try {
        await send('message.send', { content, session_id: activeSession })
      } catch {
        setIsTyping(false)
        setMessages((prev) => [...prev, { role: 'assistant', content: 'Failed to send message. Check connection.' }])
      }
    },
    [send, activeSession],
  )

  const handleNewSession = useCallback(async () => {
    try {
      const resp = await send('session.new', { name: `Session ${sessions.length + 1}` })
      if (resp.result) {
        const s = resp.result as Session
        setActiveSession(s.id)
        setMessages([])
      }
    } catch {
      // ignore
    }
  }, [send, sessions.length])

  const handleSelectSession = useCallback((id: string) => {
    setActiveSession(id)
    setMessages([])
    setIsTyping(false)
  }, [])

  return (
    <div className="flex h-screen bg-gray-950">
      <SessionSidebar
        sessions={sessions}
        activeSession={activeSession}
        onSelect={handleSelectSession}
        onCreate={handleNewSession}
        wsStatus={status}
      />

      <div className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div className="h-12 border-b border-gray-800 flex items-center px-4">
          <h2 className="text-sm font-medium text-gray-300 truncate">
            {sessions.find((s) => s.id === activeSession)?.name || 'Chat'}
          </h2>
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-3xl mx-auto">
            {messages.length === 0 && !isTyping && (
              <div className="flex flex-col items-center justify-center h-full py-20 text-center">
                <div className="text-4xl mb-4">🐾</div>
                <h3 className="text-lg font-medium text-gray-300 mb-2">Welcome to Meepo</h3>
                <p className="text-sm text-gray-500 max-w-sm">
                  Your local AI agent. Send a message to get started.
                </p>
              </div>
            )}
            {messages.map((msg, i) => (
              <ChatMessage key={i} role={msg.role} content={msg.content} />
            ))}
            {isTyping && <TypingIndicator tool={activeTool} />}
            <div ref={messagesEndRef} />
          </div>
        </div>

        {/* Input */}
        <ChatInput onSend={handleSend} disabled={status !== 'connected'} />
      </div>
    </div>
  )
}
