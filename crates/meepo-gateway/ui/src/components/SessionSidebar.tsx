import { MessageSquare, Plus, Wifi, WifiOff } from 'lucide-react'
import type { WsStatus } from '../hooks/useWebSocket'

interface Session {
  id: string
  name: string
  message_count: number
}

interface SessionSidebarProps {
  sessions: Session[]
  activeSession: string
  onSelect: (id: string) => void
  onCreate: () => void
  wsStatus: WsStatus
}

export default function SessionSidebar({
  sessions,
  activeSession,
  onSelect,
  onCreate,
  wsStatus,
}: SessionSidebarProps) {
  return (
    <div className="w-64 bg-gray-900 border-r border-gray-800 flex flex-col h-full">
      <div className="p-4 border-b border-gray-800 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-lg font-bold text-meepo-400">Meepo</span>
          {wsStatus === 'connected' ? (
            <Wifi size={14} className="text-green-500" />
          ) : (
            <WifiOff size={14} className="text-red-500" />
          )}
        </div>
        <button
          onClick={onCreate}
          className="p-1.5 rounded-lg hover:bg-gray-800 transition-colors"
          title="New session"
        >
          <Plus size={16} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {sessions.map((session) => (
          <button
            key={session.id}
            onClick={() => onSelect(session.id)}
            className={`w-full text-left px-3 py-2 rounded-lg text-sm flex items-center gap-2 transition-colors ${
              activeSession === session.id
                ? 'bg-gray-800 text-white'
                : 'text-gray-400 hover:bg-gray-800/50 hover:text-gray-200'
            }`}
          >
            <MessageSquare size={14} />
            <span className="truncate flex-1">{session.name}</span>
            {session.message_count > 0 && (
              <span className="text-xs text-gray-600">{session.message_count}</span>
            )}
          </button>
        ))}
      </div>

      <div className="p-3 border-t border-gray-800 text-xs text-gray-600 text-center">
        {wsStatus === 'connected' ? 'Connected' : wsStatus === 'connecting' ? 'Connecting...' : 'Disconnected'}
      </div>
    </div>
  )
}
