// Host stand-in for the TV's libhelpers.so.2 that imitates luna://com.webos.service.mrcu
// as LG documents it: one sample per second until setSensorInterval is called on a live
// subscription (an interval set before subscribing is ignored, the worst case).
// FAKE_MRCU=ok (default) | denied | silent | invalidtypes | sleepy (asleep for the first 1.5 s)
// | stall (the first subscription silently stops delivering after 1.5 s). Used by scripts/motion-probe-test.sh.
#include <pthread.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <math.h>
#include <time.h>
typedef struct HContext HContext;
typedef bool (*Fn)(void*, void*, HContext*);
struct HContext { Fn callback; void* userdata; void* unknown; int multiple; int pub; unsigned long ret_token; };
typedef struct { char text[2048]; } Msg;
static int interval_ms = 1000, subscribed = 0;
static HContext* sub_ctx = NULL;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static double t_start = -1;
static double mono(void){ struct timespec ts; clock_gettime(CLOCK_MONOTONIC,&ts); return ts.tv_sec+ts.tv_nsec/1e9; }
static int awake(void){ if(strcmp(getenv("FAKE_MRCU")?getenv("FAKE_MRCU"):"ok","sleepy")) return 1; return mono()-t_start > 1.5; }
static const char* mode(void){ const char* m=getenv("FAKE_MRCU"); return m?m:"ok"; }
static void reply(HContext* c, const char* s){ Msg m; snprintf(m.text,sizeof m.text,"%s",s); c->callback(NULL,&m,c); }
typedef struct { HContext* c; char uri[256]; char payload[512]; } Job;
static int stalled_once = 0;
static void* stream(void* arg){
  HContext* c=arg; double t=0; char buf[1024]; double t0=mono();
  for(;;){
    if(!strcmp(mode(),"stall") && !stalled_once && mono()-t0 > 1.5){ stalled_once=1; break; }
    pthread_mutex_lock(&mu); int live = subscribed && sub_ctx==c; int iv=interval_ms; pthread_mutex_unlock(&mu);
    if(!live) break;
    usleep(iv*1000); t+=iv/1000.0;
    double a=0.3*sin(t*2.0); // twisting slowly
    snprintf(buf,sizeof buf,"{\"returnValue\":true,\"interval\":%d.0,\"coordinate\":{\"x\":960,\"y\":540},"
      "\"gyroscope\":{\"x\":0.0,\"y\":%f,\"z\":0.0},\"accelerometer\":{\"x\":0.1,\"y\":0.2,\"z\":9.8},"
      "\"linearAcceleration\":{\"x\":0,\"y\":0,\"z\":0},\"gameRotationVector\":{\"x\":0,\"y\":%f,\"z\":0,\"w\":%f}}",
      iv, 0.6*cos(t*2.0), sin(a/2), cos(a/2));
    pthread_mutex_lock(&mu); live = subscribed && sub_ctx==c; pthread_mutex_unlock(&mu);
    if(live) reply(c,buf);
  }
  return NULL;
}
static void* worker(void* arg){
  Job* j=arg; HContext* c=j->c; usleep(3000);
  if(!strcmp(mode(),"silent")) { free(j); return NULL; }
  if(!strcmp(mode(),"denied")) { reply(c,"{\"returnValue\":false,\"errorCode\":-1,\"errorText\":\"Denied method call \\\"x\\\" for category \\\"/sensor2\\\"\"}"); free(j); return NULL; }
  const char* u=strstr(j->uri,"mrcu/"); u=u?u+5:j->uri;
  char buf[512];
  if(!strcmp(u,"sensor2/getSensorState")) reply(c,awake()?"{\"isAlive\":true,\"returnValue\":true}":"{\"isAlive\":false,\"returnValue\":true}");
  else if(!strcmp(u,"sensor2/getSensorInterval")){ snprintf(buf,sizeof buf,"{\"minInterval\":10,\"returnValue\":true,\"interval\":%d,\"maxInterval\":1000}",interval_ms); reply(c,buf);}
  else if(!strcmp(u,"sensor2/setSensorInterval")){
    int v=atoi(strstr(j->payload,":")+1);
    pthread_mutex_lock(&mu); if(subscribed) interval_ms=v; pthread_mutex_unlock(&mu); // ignored while not subscribed
    reply(c,"{\"returnValue\":true}");
  } else if(!strcmp(u,"sensor2/getSensorEventData")){
    if(!strcmp(mode(),"invalidtypes") && !strstr(j->payload,"\"all\"")) { reply(c,"{\"returnValue\":false,\"errorCode\":1002,\"errorText\":\"Invalid parameter\"}"); free(j); return NULL; }
    if(!awake()) { reply(c,"{\"returnValue\":false,\"errorCode\":1003,\"errorText\":\"Magic remote is not ready\"}"); free(j); return NULL; }
    pthread_mutex_lock(&mu); if(subscribed){ pthread_mutex_unlock(&mu); reply(c,"{\"returnValue\":false,\"errorCode\":1004,\"errorText\":\"Already add subscription\"}"); free(j); return NULL;}
    subscribed=1; sub_ctx=c; interval_ms=1000; pthread_mutex_unlock(&mu);
    reply(c,"{\"subscribed\":true,\"returnValue\":true}");
    pthread_t t; pthread_create(&t,NULL,stream,c); pthread_detach(t);
  } else if(!strcmp(u,"sensor2/cancelSensorDataSubscribe")){ pthread_mutex_lock(&mu); subscribed=0; sub_ctx=NULL; pthread_mutex_unlock(&mu); reply(c,"{\"returnValue\":true}"); }
  else if(!strcmp(u,"sensor2/resetQuaternion")) reply(c,"{\"returnValue\":true}");
  else reply(c,"{\"returnValue\":false,\"errorCode\":-1,\"errorText\":\"Unknown method\"}");
  free(j); return NULL;
}
int HLunaServiceCall(const char* uri,const char* payload,HContext* c){
  if(t_start<0) t_start=mono();
  Job* j=calloc(1,sizeof *j); j->c=c; snprintf(j->uri,sizeof j->uri,"%s",uri); snprintf(j->payload,sizeof j->payload,"%s",payload);
  pthread_t t; pthread_create(&t,NULL,worker,j); pthread_detach(t); return 0;
}
int HUnregisterServiceCallback(HContext* c){ pthread_mutex_lock(&mu); if(sub_ctx==c){subscribed=0;sub_ctx=NULL;} pthread_mutex_unlock(&mu); return 0; }
const char* HLunaServiceMessage(void* m){ return ((Msg*)m)->text; }
const char* HGetError(int e){ return "fake"; }
