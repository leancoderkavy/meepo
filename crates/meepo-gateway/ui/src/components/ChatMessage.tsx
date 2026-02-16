import { Bot, User } from 'lucide-react'

interface ChatMessageProps {
  role: 'user' | 'assistant'
  content: string
}

export default function ChatMessage({ role, content }: ChatMessageProps) {
  const isUser = role === 'user'

  return (
    <div className={`flex gap-3 px-4 py-3 ${isUser ? '' : 'bg-gray-900/50'}`}>
      <div
        className={`flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center ${
          isUser ? 'bg-meepo-700' : 'bg-gray-700'
        }`}
      >
        {isUser ? <User size={16} /> : <Bot size={16} />}
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-xs text-gray-500 mb-1 font-medium">
          {isUser ? 'You' : 'Meepo'}
        </div>
        <div className="markdown-body text-sm text-gray-200 whitespace-pre-wrap break-words">
          {content}
        </div>
      </div>
    </div>
  )
}
