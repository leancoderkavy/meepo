import { Bot } from 'lucide-react'

interface TypingIndicatorProps {
  tool?: string
}

export default function TypingIndicator({ tool }: TypingIndicatorProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-gray-900/50">
      <div className="flex-shrink-0 w-8 h-8 rounded-full flex items-center justify-center bg-gray-700">
        <Bot size={16} />
      </div>
      <div className="flex items-center gap-2">
        {tool ? (
          <span className="text-xs text-meepo-400 animate-pulse">Running {tool}...</span>
        ) : (
          <div className="flex gap-1">
            <div className="w-2 h-2 rounded-full bg-gray-500 typing-dot" />
            <div className="w-2 h-2 rounded-full bg-gray-500 typing-dot" />
            <div className="w-2 h-2 rounded-full bg-gray-500 typing-dot" />
          </div>
        )}
      </div>
    </div>
  )
}
