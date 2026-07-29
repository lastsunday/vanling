import i18n from 'i18next';
import Backend from 'i18next-http-backend';
import { initReactI18next } from 'react-i18next';
import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import 'dayjs/locale/zh-cn';

dayjs.extend(relativeTime);
dayjs.locale('zh-cn');
i18n.on('languageChanged', (lng) => {
  dayjs.locale(lng === 'zh' ? 'zh-cn' : 'en');
});

i18n
  .use(Backend)
  .use(initReactI18next)
  .init({
    fallbackLng: 'zh',
    debug: import.meta.env.DEV,
    backend: {
      loadPath: '/assets/locales/{{lng}}/{{ns}}.json',
    }
  });

export default i18n;
