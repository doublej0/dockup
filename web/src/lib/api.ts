export function getApiBase(): string {
  if (typeof window === 'undefined') {
    return process.env.INTERNAL_API_URL || process.env.PUBLIC_API_URL || 'http://localhost:3101';
  }
  return import.meta.env.PUBLIC_API_URL || 'http://localhost:3101';
}

export interface Client {
  id: string;
  name: string;
  host: string;
  color: string;
  compose_file_path: string | null;
  agent_version: string | null;
  agent_update_mode: string;
  last_seen: string | null;
  connected: boolean;
  created_at: string;
  updates_available: number;
}

export interface Container {
  id: string;
  client_id: string;
  container_name: string;
  image: string;
  current_digest: string | null;
  latest_digest: string | null;
  update_available: boolean;
  update_mode: string;
  status: string;
  checked_at: string | null;
}

export interface UpdateJob {
  id: string;
  client_id: string;
  container_name: string;
  image: string;
  from_digest: string | null;
  to_digest: string | null;
  status: string;
  output: string | null;
  started_at: string;
  completed_at: string | null;
}

export interface OnboardClientRequest {
  name: string;
  host: string;
  color: string;
  compose_file_path?: string;
  ssh_user: string;
  ssh_password: string;
  agent_update_mode: string;
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${getApiBase()}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`API ${path} ${res.status}: ${text}`);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export async function getClients(): Promise<Client[]> {
  return apiFetch<Client[]>('/api/clients');
}

export async function getClient(id: string): Promise<Client> {
  return apiFetch<Client>(`/api/clients/${id}`);
}

export async function updateClient(id: string, data: Partial<Client>): Promise<Client> {
  return apiFetch<Client>(`/api/clients/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteClient(id: string): Promise<void> {
  return apiFetch<void>(`/api/clients/${id}`, { method: 'DELETE' });
}

export async function getContainers(clientId: string, showStopped = false): Promise<Container[]> {
  return apiFetch<Container[]>(
    `/api/clients/${clientId}/containers?show_stopped=${showStopped}`
  );
}

export async function updateContainer(
  clientId: string,
  name: string,
  data: { update_mode: string }
): Promise<Container> {
  return apiFetch<Container>(`/api/clients/${clientId}/containers/${name}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function triggerUpdate(
  clientId: string,
  containerNames?: string[],
  all?: boolean
): Promise<UpdateJob[]> {
  return apiFetch<UpdateJob[]>(`/api/clients/${clientId}/update`, {
    method: 'POST',
    body: JSON.stringify({ container_names: containerNames, all }),
  });
}

export async function getJobs(clientId: string): Promise<UpdateJob[]> {
  return apiFetch<UpdateJob[]>(`/api/clients/${clientId}/jobs`);
}

export async function getJob(jobId: string): Promise<UpdateJob> {
  return apiFetch<UpdateJob>(`/api/jobs/${jobId}`);
}

export async function getRecentJobs(params?: {
  client_id?: string;
  status?: string;
  page?: number;
  per_page?: number;
}): Promise<UpdateJob[]> {
  const q = new URLSearchParams();
  if (params?.client_id) q.set('client_id', params.client_id);
  if (params?.status) q.set('status', params.status);
  if (params?.page) q.set('page', String(params.page));
  if (params?.per_page) q.set('per_page', String(params.per_page));
  const qs = q.toString();
  return apiFetch<UpdateJob[]>(`/api/jobs${qs ? `?${qs}` : ''}`);
}

export async function onboardClient(data: OnboardClientRequest): Promise<Client> {
  return apiFetch<Client>('/api/onboard', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}
