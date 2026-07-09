import {
  Box,
  Burger,
  Divider,
  Drawer,
  Flex,
  Group,
  ScrollArea,
  Text
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useTranslation } from 'react-i18next';
import logo from '../../assets/logo.svg';
import { LanguageSwitcher } from '../LanguageSwitcher';
import classes from './WebsiteHeader.module.css';

export function WebsiteHeader() {
  const [drawerOpened, { toggle: toggleDrawer, close: closeDrawer }] = useDisclosure(false);
  const { t } = useTranslation();

  return (
    <Box pb={120}>
      <header className={classes.header}>
        <Group justify="space-between" h="100%">
          <Group gap="xs">
            <img className={`${classes.logo}`} src={logo}></img>
            <Text>{t('title')}</Text>
          </Group>
          <Group h="100%" gap={0} visibleFrom="sm">
            <a href="#" className={classes.link}>
              {t('menu.home')}
            </a>
            <LanguageSwitcher></LanguageSwitcher>
          </Group>

          <Burger opened={drawerOpened} onClick={toggleDrawer} hiddenFrom="sm" />
        </Group>
      </header>

      <Drawer
        opened={drawerOpened}
        onClose={closeDrawer}
        size="100%"
        padding="md"
        title={t('navigator')}
        hiddenFrom="sm"
      >

        <ScrollArea h="calc(100vh - 80px" mx="-md">
          <Divider my="sm" />
          <a href="#" className={classes.link}>
            {t('menu.home')}
          </a>
          <Flex justify="end">
            <LanguageSwitcher></LanguageSwitcher>
          </Flex>
        </ScrollArea>
      </Drawer>
    </Box >
  );
}
