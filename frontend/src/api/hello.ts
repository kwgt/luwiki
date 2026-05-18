import axios from 'axios';

const helloClient = axios.create({
  baseURL: '/api',
  timeout: 0,
});

export async function ensureHelloAuth(): Promise<void> {
  await helloClient.get('/hello', {
    responseType: 'text',
  });
}
