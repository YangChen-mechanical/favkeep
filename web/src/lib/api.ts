import { authApi } from './api/auth';
import { configApi } from './api/config';
import { mediaApi } from './api/media';
import { platformsApi } from './api/platforms';
import { sourcesApi } from './api/sources';
import { systemApi } from './api/system';
import { watchApi } from './api/watch';

localStorage.removeItem('authToken');

export const apiClient = {
	...authApi,
	...mediaApi,
	...watchApi,
	...sourcesApi,
	...configApi,
	...platformsApi,
	...systemApi
};

export default apiClient;
