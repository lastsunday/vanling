import { useTranslation } from 'react-i18next';
import type { SessionRound } from '@/data/session';
import { Box, Tooltip } from '@mantine/core';

interface SessionMinimapProps {
  rounds: SessionRound[];
  onRoundClick: (index: number) => void;
}

export function SessionMinimap({ rounds, onRoundClick }: SessionMinimapProps) {
  const { t } = useTranslation();
  if (rounds.length === 0) return null;

  return (
    <Box style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
      {rounds.map((r, i) => {
        return (
          <Tooltip key={r.round_id} label={t('sessions.round_label', { number: i + 1 })}>
            <Box
              style={{
                width: 28,
                height: 28,
                borderRadius: 6,
                background: '#40c057',
                opacity: 0.75,
                cursor: 'pointer',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                transition: 'opacity 0.12s',
                fontWeight: 600,
                fontSize: 13,
                color: '#fff',
                textShadow: '0 1px 2px rgba(0,0,0,0.3)',
              }}
              onMouseEnter={(e) => { e.currentTarget.style.opacity = '1'; }}
              onMouseLeave={(e) => { e.currentTarget.style.opacity = '0.75'; }}
              onClick={(e) => {
                e.stopPropagation();
                onRoundClick(i);
              }}
            >
              {i + 1}
            </Box>
          </Tooltip>
        );
      })}
    </Box>
  );
}
