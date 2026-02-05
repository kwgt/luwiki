import { apiClient } from './client';

export interface CurrentUserResponse {
  id: string;
  username: string;
  display_name: string;
  timestamp: string;
}

export async function fetchCurrentUser(): Promise<CurrentUserResponse> {
  const res = await apiClient.get<CurrentUserResponse>('/users/me');
  return res.data;
}
