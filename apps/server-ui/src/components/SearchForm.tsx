import type { GroupProps } from '@mantine/core';
import { Button, Group } from '@mantine/core';
import type { ReactNode } from 'react';

interface SearchFormProps {
  onSubmit: () => void;
  submitLabel: string;
  children?: ReactNode;
  extra?: ReactNode;
  groupProps?: GroupProps;
}

export function SearchForm({ onSubmit, submitLabel, children, extra, groupProps }: SearchFormProps) {
  return (
    <form onSubmit={(e) => { e.preventDefault(); onSubmit(); }}>
      <Group {...groupProps}>
        {children}
        {extra}
        <Button type="submit">{submitLabel}</Button>
      </Group>
    </form>
  );
}
