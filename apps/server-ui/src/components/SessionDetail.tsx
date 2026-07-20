import { getSessionRounds, listRoundData } from '@/api';
import { SessionMinimap } from '@/components/SessionMinimap';
import { Timeline } from '@/components/Timeline';
import type { RoundData } from '@/data/round-data';
import type { TurnStep } from '@/data/session';
import {
  Badge,
  Box,
  Button,
  CopyButton,
  Group,
  Paper,
  Text,
} from '@mantine/core';
import { useQueries, useQuery } from '@tanstack/react-query';
import dayjs from 'dayjs';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { IconChevronDown, IconChevronRight } from '@tabler/icons-react';

const stepColors: Record<string, string> = {
  input_audio: 'green',
  input_audio_tail: 'lime',
  asr: 'cyan',
  text: 'yellow',
  llm: 'violet',
  tts: 'red',
};

const barHexColors: Record<string, string> = {
  input_audio: '#40c057',
  input_audio_tail: '#69db7c',
  asr: '#15aabf',
  text: '#fab005',
  llm: '#7950f2',
  tts: '#fa5252',
};

function stepValueMs(s: TurnStep): number | null {
  if (s.step === 'input_audio' || s.step === 'input_audio_tail' || s.step === 'tts') {
    return s.audio_duration_ms;
  }
  return s.duration_ms;
}

function RoundSummary({ steps }: { steps: TurnStep[] }) {
  const { t } = useTranslation();
  const valid = steps.filter((s) => {
    const v = stepValueMs(s);
    return v != null && s.has_data;
  });
  if (valid.length === 0) return null;

  const processSteps = steps.filter((s) =>
    s.step !== 'input_audio' && s.step !== 'input_audio_tail' && s.step !== 'text'
    && s.has_data && s.duration_ms != null && s.duration_ms >= 0,
  );
  const totalProcessMs = processSteps.reduce((sum, s) => sum + s.duration_ms!, 0);

  const runSteps = steps.filter((s) =>
    s.step !== 'text' && s.has_data,
  ).filter((s) => {
    const v = s.step === 'input_audio' || s.step === 'input_audio_tail' || s.step === 'tts'
      ? s.audio_duration_ms : s.duration_ms;
    return v != null && v >= 0;
  });
  const totalRunMs = runSteps.reduce((sum, s) => {
    return sum + (s.step === 'input_audio' || s.step === 'input_audio_tail' || s.step === 'tts'
      ? (s.audio_duration_ms ?? 0) : (s.duration_ms ?? 0));
  }, 0);

  return (
    <>
      <Group gap={4} pl="lg" wrap="wrap">
        {valid.map((s, i) => {
          const v = stepValueMs(s)!;
          const proc = s.duration_ms != null
            ? `⏱${(s.duration_ms / 1000).toFixed(s.duration_ms < 1000 ? 1 : 0)}s`
            : '';
          const label = t(`sessions.step.${s.step}`);
          const dur = `${(v / 1000).toFixed(v < 1000 ? 1 : 0)}s`;

          let badgeText: string;
          if (s.step === 'input_audio' || s.step === 'input_audio_tail') {
            badgeText = `${label} ${dur}`;
          } else if (s.step === 'tts') {
            badgeText = `${label} ${dur}${proc ? ' ' + proc : ''}`;
          } else {
            const txt = s.text
              ? ` "${s.text.slice(0, 20)}${s.text.length > 20 ? '...' : ''}"`
              : '';
            const modeTag = s.mode === 'wake' ? ` [${t('sessions.mode.wake')}]` : '';
            badgeText = `${label}${modeTag}${txt}${proc ? ' ' + proc : ''}`;
          }

          return (
            <Group key={s.step} gap={2} wrap="nowrap" style={{ flexShrink: 0 }}>
              {i > 0 && <Text c="dimmed" size="xs">→</Text>}
              <Badge
                color={stepColors[s.step] ?? 'gray'}
                variant="light"
                size="sm"
                style={{ textTransform: 'none', fontWeight: 400, fontSize: 10 }}
              >
                {badgeText}
              </Badge>
            </Group>
          );
        })}
      </Group>
      {totalProcessMs > 0 && (
        <>
          <Box mt={4} mb={6} ml="lg" style={{ borderTop: '1px dashed var(--mantine-color-gray-3)' }} />
          <Box ml="lg">
            <Group gap={8} mb={2} wrap="nowrap">
              <Text style={{ width: 56, flexShrink: 0 }} c="dimmed" size="xs">{t('sessions.latency_waterfall')}</Text>
              <Box style={{ display: 'flex', flex: 1, height: 14, borderRadius: 3, overflow: 'hidden', background: 'var(--mantine-color-gray-1)' }}>
                {processSteps.map((s) => {
                  const pct = totalProcessMs > 0 ? ((s.duration_ms ?? 0) / totalProcessMs) * 100 : 0;
                  return (
                    <Box
                      key={s.step}
                      style={{ width: `${Math.max(pct, 0)}%`, height: '100%', background: barHexColors[s.step] ?? '#868e96', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
                    >
                      {pct >= 15 && (
                        <Text size="xs" c="white" fw={600} style={{ lineHeight: '14px', textShadow: '0 1px 2px rgba(0,0,0,0.3)' }}>
                          {Math.round(pct)}%
                        </Text>
                      )}
                    </Box>
                  );
                })}
              </Box>
              <Text size="xs" fw={600} style={{ whiteSpace: 'nowrap' }}>
                {(totalProcessMs / 1000).toFixed(totalProcessMs < 1000 ? 1 : 0)}s
              </Text>
            </Group>
            <Group gap={8} wrap="nowrap">
              <Text style={{ width: 56, flexShrink: 0 }} c="dimmed" size="xs">{t('sessions.runtime')}</Text>
              <Box style={{ display: 'flex', flex: 1, height: 14, borderRadius: 3, overflow: 'hidden', background: 'var(--mantine-color-gray-1)' }}>
                {runSteps.map((s) => {
                  const v = s.step === 'input_audio' || s.step === 'tts'
                    ? (s.audio_duration_ms ?? 0) : (s.duration_ms ?? 0);
                  const pct = totalRunMs > 0 ? (v / totalRunMs) * 100 : 0;
                  return (
                    <Box
                      key={s.step}
                      style={{ width: `${Math.max(pct, 0)}%`, height: '100%', background: barHexColors[s.step] ?? '#868e96', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
                    >
                      {pct >= 15 && (
                        <Text size="xs" c="white" fw={600} style={{ lineHeight: '14px', textShadow: '0 1px 2px rgba(0,0,0,0.3)' }}>
                          {Math.round(pct)}%
                        </Text>
                      )}
                    </Box>
                  );
                })}
              </Box>
              <Text size="xs" fw={600} style={{ whiteSpace: 'nowrap' }}>
                {(totalRunMs / 1000).toFixed(totalRunMs < 1000 ? 1 : 0)}s
              </Text>
            </Group>
          </Box>
        </>
      )}
    </>
  );
}

interface SessionDetailProps {
  sessionId: string;
}

export function SessionDetail({ sessionId }: SessionDetailProps) {
  const { t } = useTranslation();

  const [expandedRoundMap, setExpandedRoundMap] = useState<Record<string, boolean>>({});
  const roundElementsRef = useRef<Record<string, HTMLElement | null>>({});
  const prevRoundIdsRef = useRef<string>('');

  const { data: rounds = [] } = useQuery({
    queryKey: ['session-rounds', sessionId],
    queryFn: () => getSessionRounds(sessionId),
  });

  const roundDataQueries = useQueries({
    queries: rounds.map((r) => ({
      queryKey: ['round-data', r.round_id],
      queryFn: () => listRoundData(r.round_id),
    })),
  });

  const roundDataMap = useMemo(() => {
    const map: Record<string, RoundData[]> = {};
    rounds.forEach((r, i) => {
      map[r.round_id] = roundDataQueries[i]?.data ?? [];
    });
    return map;
  }, [rounds, roundDataQueries]);

  useEffect(() => {
    const currentIds = rounds.map((r) => r.round_id).join(',');
    if (currentIds !== prevRoundIdsRef.current) {
      prevRoundIdsRef.current = currentIds;
      setExpandedRoundMap((prev) => {
        const next: Record<string, boolean> = {};
        rounds.forEach((r) => {
          next[r.round_id] = prev[r.round_id] ?? false;
        });
        return next;
      });
    }
  }, [rounds]);

  const toggleRound = (roundId: string) => {
    setExpandedRoundMap((prev) => ({ ...prev, [roundId]: !prev[roundId] }));
  };

  const scrollToRound = (index: number) => {
    const round = rounds[index];
    if (!round) return;
    toggleRound(round.round_id);
    setTimeout(() => {
      roundElementsRef.current[round.round_id]?.scrollIntoView({
        behavior: 'smooth',
        block: 'start',
      });
    }, 0);
  };

  const totalMs = rounds.reduce(
    (sum, r) => sum + r.steps.reduce((s, st) => s + (st.duration_ms ?? 0), 0),
    0,
  );

  const allDataReady = rounds.length > 0
    && roundDataQueries.every((q) => q.data !== undefined);

  return (
    <Paper withBorder shadow="sm" p="md" radius="md">
      <Group justify="space-between" mb="md">
        <Group gap="xs">
          <CopyButton value={sessionId}>
            {({ copied, copy }) => (
              <Group gap={2} wrap="nowrap">
                <Text size="sm" fw={600} style={{ fontFamily: 'monospace' }}>
                  {sessionId}
                </Text>
                <Button variant="subtle" size="compact-xs" onClick={copy} px={4}>
                  {copied ? '✓' : t('sessions.detail.copy')}
                </Button>
              </Group>
            )}
          </CopyButton>
        </Group>
        <Group gap="md">
          <Text size="xs" c="dimmed">
            {t('sessions.turn_count', { count: rounds.length })}
          </Text>
          {totalMs > 0 && (
            <Text size="xs" c="dimmed">
              {(totalMs / 1000).toFixed(totalMs < 1000 ? 1 : 0)}s
            </Text>
          )}
          <Text size="xs" c="dimmed">
            {rounds[0]?.create_datetime
              ? dayjs(rounds[0].create_datetime).format('YYYY-MM-DD HH:mm:ss')
              : ''}
          </Text>
        </Group>
      </Group>

      {rounds.length > 1 && (
        <Box mb="md">
          <SessionMinimap rounds={rounds} onRoundClick={scrollToRound} />
        </Box>
      )}

      {!allDataReady && (
        <Text size="sm" c="dimmed" ta="center" py="xl">
          {t('loading')}
        </Text>
      )}

      {allDataReady && rounds.map((round, idx) => {
        const isExpanded = expandedRoundMap[round.round_id] ?? false;
        return (
          <Box
            key={round.round_id}
            mb="lg"
            ref={(el) => { roundElementsRef.current[round.round_id] = el; }}
          >
            <Group
              gap="xs"
              mb={isExpanded ? 4 : 0}
              style={{ cursor: 'pointer' }}
              onClick={() => toggleRound(round.round_id)}
            >
              {isExpanded ? (
                <IconChevronDown size={14} style={{ flexShrink: 0, color: 'var(--mantine-color-gray-5)' }} />
              ) : (
                <IconChevronRight size={14} style={{ flexShrink: 0, color: 'var(--mantine-color-gray-5)' }} />
              )}
              <Text size="sm" fw={600}>
                {t('sessions.round_label', { number: idx + 1 })}
              </Text>
              <Text size="xs" c="dimmed">
                {round.create_datetime
                  ? dayjs(round.create_datetime).format('HH:mm:ss')
                  : ''}
              </Text>
            </Group>
            {isExpanded ? (
              <>
                <Timeline
                  roundId={round.round_id}
                  dataItems={roundDataMap[round.round_id] ?? []}
                />
              </>
            ) : (
              <RoundSummary steps={round.steps} />
            )}
          </Box>
        );
      })}
    </Paper>
  );
}
